use crate::{
    config::Config,
    connection_manager::ConnectionManager,
    database_client::DatabaseClient,
    pinger::FinalizedPing,
    session_handler::SessionHandler,
    task_supervisor::{
        ClientUpdate, HealthReport, SupervisorConfig, SupervisorShutdown, TaskSupervisor,
    },
};
use anyhow::Result;
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::net::{IpAddr, TcpListener};
use std::sync::Arc;
use tokio::{
    sync::{Notify, mpsc, oneshot, watch},
    task::JoinHandle,
};
use zzping_proto::zzping::CollectorRole;

/// The configuration that is persisted to disk to allow for continued
/// operation if the database is unavailable on startup.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CachedIntent {
    pub targets: HashSet<IpAddr>,
    pub ping_rate_pps: u64,
}

/// The long-lived root of the application.
pub struct CollectorService {
    config: Config,
    // This is an option so we can `take` it in the run method.
    task_supervisor: Option<TaskSupervisor>,
    _lock: TcpListener,
    // The handle to the supervisor task, so we can await it during shutdown.
    supervisor_handle: Option<JoinHandle<Result<()>>>,
    // The sender for the shutdown signal to the supervisor.
    supervisor_shutdown_tx: Option<mpsc::Sender<SupervisorShutdown>>,
    // A channel to send the supervisor_shutdown_tx to the test harness.
    test_shutdown_tx_sender: Option<mpsc::Sender<mpsc::Sender<SupervisorShutdown>>>,
}

impl CollectorService {
    /// Creates a new `CollectorService`.
    pub fn new(config: Config, lock: TcpListener) -> Result<Self> {
        // Default health interval is 1000ms
        let task_supervisor = TaskSupervisor::new(config.collector_uuid.clone(), None, 1000);
        Ok(Self {
            config,
            task_supervisor: Some(task_supervisor),
            _lock: lock,
            supervisor_handle: None,
            supervisor_shutdown_tx: None,
            test_shutdown_tx_sender: None,
        })
    }

    /// Creates a new `CollectorService` for testing, with channels for injecting data and commands.
    pub fn new_for_test(
        config: Config,
        lock: TcpListener,
        test_data_tx_sender: Option<mpsc::Sender<mpsc::Sender<FinalizedPing>>>,
        test_shutdown_tx_sender: Option<mpsc::Sender<mpsc::Sender<SupervisorShutdown>>>,
    ) -> Result<Self> {
        // For tests allow passing the test_data_tx_sender and use the standard
        // default interval of 1000ms. Tests that need a faster interval should
        // call `new_for_test_with_interval` below.
        let task_supervisor =
            TaskSupervisor::new(config.collector_uuid.clone(), test_data_tx_sender, 1000);
        Ok(Self {
            config,
            task_supervisor: Some(task_supervisor),
            _lock: lock,
            supervisor_handle: None,
            supervisor_shutdown_tx: None,
            test_shutdown_tx_sender,
        })
    }

    /// Test helper which allows specifying a custom health interval (ms).
    pub fn new_for_test_with_interval(
        config: Config,
        lock: TcpListener,
        test_data_tx_sender: Option<mpsc::Sender<mpsc::Sender<FinalizedPing>>>,
        test_shutdown_tx_sender: Option<mpsc::Sender<mpsc::Sender<SupervisorShutdown>>>,
        health_interval_ms: u64,
    ) -> Result<Self> {
        let task_supervisor = TaskSupervisor::new(
            config.collector_uuid.clone(),
            test_data_tx_sender,
            health_interval_ms,
        );
        Ok(Self {
            config,
            task_supervisor: Some(task_supervisor),
            _lock: lock,
            supervisor_handle: None,
            supervisor_shutdown_tx: None,
            test_shutdown_tx_sender,
        })
    }

    /// Initiates a graceful shutdown of the collector and all its tasks.
    pub async fn shutdown(&mut self) -> Result<()> {
        info!("CollectorService shutdown initiated.");

        // If the shutdown sender exists, send the shutdown command.
        if let Some(shutdown_tx) = self.supervisor_shutdown_tx.take() {
            let (ack_tx, ack_rx) = oneshot::channel();
            info!("Sending shutdown command to TaskSupervisor...");
            if shutdown_tx
                .send(SupervisorShutdown { ack_sender: ack_tx })
                .await
                .is_err()
            {
                warn!(
                    "Failed to send shutdown command to TaskSupervisor. It may have already exited."
                );
            } else {
                // Wait for the supervisor to acknowledge the shutdown.
                if ack_rx.await.is_err() {
                    warn!("TaskSupervisor did not acknowledge shutdown. It may have panicked.");
                }
            }
        }

        // Wait for the supervisor task to fully complete.
        if let Some(handle) = self.supervisor_handle.take() {
            info!("Awaiting TaskSupervisor completion...");
            if let Err(e) = handle.await? {
                warn!("TaskSupervisor exited with an error: {}", e);
            }
        }

        info!("CollectorService shutdown complete.");
        Ok(())
    }

    /// Runs the `CollectorService` to completion, including signal handling.
    pub async fn run(mut self) -> Result<()> {
        info!("CollectorService running. Press Ctrl+C to exit.");
        use std::time::Duration;
        use tokio::signal::unix::{SignalKind, signal};
        use tokio::time::timeout;

        let mut sigterm = signal(SignalKind::terminate())?;

        tokio::select! {
            res = self.run_internal() => {
                if let Err(e) = res {
                    warn!("Collector service exited with an error: {e}");
                } else {
                    info!("Collector service exited gracefully.");
                }
            },
            _ = tokio::signal::ctrl_c() => {
                info!("Received SIGINT. Initiating graceful shutdown.");
                if timeout(Duration::from_secs(10), self.shutdown()).await.is_err() {
                    warn!("Graceful shutdown timed out. Exiting forcefully.");
                } else {
                    info!("Graceful shutdown complete.");
                }
            },
            _ = sigterm.recv() => {
                info!("Received SIGTERM. Initiating graceful shutdown.");
                if timeout(Duration::from_secs(10), self.shutdown()).await.is_err() {
                    warn!("Graceful shutdown timed out. Exiting forcefully.");
                } else {
                    info!("Graceful shutdown complete.");
                }
            },
        }

        Ok(())
    }

    /// The main internal loop of the `CollectorService`.
    async fn run_internal(&mut self) -> Result<()> {
        let (client_tx, mut client_rx) = mpsc::channel::<DatabaseClient>(1);
        let (config_tx, config_rx) = watch::channel::<Option<SupervisorConfig>>(None);
        let (client_update_tx, client_update_rx) = mpsc::channel::<ClientUpdate>(10);
        let (health_report_tx, mut health_report_rx) = mpsc::channel::<HealthReport>(10);
        let (latest_health_tx, latest_health_rx) = watch::channel(HealthReport {
            total_buffer_size: 0,
            role: CollectorRole::Standby,
            fatal_errors: vec![],
        });
        let (persistence_tx, mut persistence_rx) = mpsc::channel::<CachedIntent>(1);
        let (supervisor_shutdown_tx, supervisor_shutdown_rx) =
            mpsc::channel::<SupervisorShutdown>(1);

        self.supervisor_shutdown_tx = Some(supervisor_shutdown_tx.clone());
        if let Some(sender) = &self.test_shutdown_tx_sender
            && sender.send(supervisor_shutdown_tx).await.is_err()
        {
            warn!("Failed to send supervisor_shutdown_tx to test harness. Test might hang.");
        }
        let reconnect_notify = Arc::new(Notify::new());

        // Persistence task
        tokio::spawn(async move {
            while let Some(intent) = persistence_rx.recv().await {
                if let Ok(ron_string) = ron::to_string(&intent)
                    && let Err(e) = tokio::fs::write("last_intent.ron", ron_string).await
                {
                    warn!("Failed to write last_intent.ron: {e}");
                }
            }
        });

        // ConnectionManager task
        let connection_manager = ConnectionManager::new(
            self.config.database_addr.clone(),
            self.config.auth_token.clone(),
            client_tx,
            reconnect_notify.clone(),
        );
        tokio::spawn(connection_manager.run());

        // TaskSupervisor task
        let supervisor = self.task_supervisor.take().expect("No supervisor");
        self.supervisor_handle = Some(tokio::spawn(supervisor.run(
            config_rx,
            client_update_rx,
            health_report_tx,
            supervisor_shutdown_rx,
        )));

        // Health forwarder task
        let health_forwarder_handle = tokio::spawn(async move {
            while let Some(report) = health_report_rx.recv().await {
                debug!("Forwarding new health report: {report:?}");
                latest_health_tx.send(report).ok();
            }
        });

        // Main session loop
        let supervisor_handle = self
            .supervisor_handle
            .as_mut()
            .expect("Supervisor handle should exist");

        tokio::select! {
            _ = async {
                loop {
                    if let Some(client) = client_rx.recv().await {
                        client_update_tx.send(ClientUpdate::NewClient(Box::new(client.clone()))).await.ok();
                        let session_handler = SessionHandler::new(
                            client,
                            config_tx.clone(),
                            self.config.collector_uuid.clone(),
                            latest_health_rx.clone(),
                            persistence_tx.clone(),
                        );
                        let session_handle = tokio::spawn(session_handler.run());

                        if let Err(e) = session_handle.await {
                            warn!("Session ended with an error: {:?}", e);
                        }

                        client_update_tx.send(ClientUpdate::ClientLost).await.ok();
                        reconnect_notify.notify_one();
                    } else {
                        warn!("ConnectionManager has shut down.");
                        break;
                    }
                }
            } => {
                warn!("Main session loop exited unexpectedly.");
            },
            res = supervisor_handle => {
                match res {
                    Ok(Ok(_)) => info!("TaskSupervisor exited gracefully."),
                    Ok(Err(e)) => warn!("TaskSupervisor exited with an error: {}", e),
                    Err(e) => warn!("TaskSupervisor task panicked: {}", e),
                }
            }
        }

        health_forwarder_handle.abort();
        info!("CollectorService internal loop has shut down.");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ntest::timeout;

    fn mock_config() -> Config {
        Config {
            collector_uuid: "test-uuid".to_string(),
            database_addr: "http://127.0.0.1:0".to_string(),
            auth_token: "test-token".to_string(),
        }
    }

    #[tokio::test]
    #[timeout(100)]
    async fn test_collector_service_new() {
        let config = mock_config();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let service = CollectorService::new(config, listener);
        assert!(service.is_ok());
    }
}
