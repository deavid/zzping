//! High-level collector service orchestration.

use crate::{
    client_holder::ClientHolder,
    config::Config,
    connection_manager::ConnectionManager,
    database_client::DatabaseClientTrait,
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
    /// Set of target IPs managed by this collector.
    pub targets: HashSet<IpAddr>,
    /// Desired pings per second across targets.
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
    // Optional holder that is kept up-to-date with the current DatabaseClient.
    client_holder: Option<ClientHolder>,
    // Path to the cache file for persisting intent. None means no caching.
    cache_file_path: Option<String>,
}

impl CollectorService {
    /// Creates a new `CollectorService`.
    pub fn new(
        config: Config,
        lock: TcpListener,
        cached_intent: Option<CachedIntent>,
    ) -> Result<Self> {
        // Default health interval is 1000ms
        let task_supervisor = TaskSupervisor::new_with_worker_count_tx(
            config.collector_uuid.clone(),
            1000,
            cached_intent,
            None,
        );
        Ok(Self {
            config,
            task_supervisor: Some(task_supervisor),
            _lock: lock,
            supervisor_handle: None,
            supervisor_shutdown_tx: None,
            test_shutdown_tx_sender: None,
            client_holder: None,
            cache_file_path: Some("last_intent.ron".to_string()),
        })
    }

    /// Creates a new `CollectorService` for testing, with channels for injecting commands.
    pub fn new_for_test(
        config: Config,
        lock: TcpListener,
        test_shutdown_tx_sender: Option<mpsc::Sender<mpsc::Sender<SupervisorShutdown>>>,
        cached_intent: Option<CachedIntent>,
    ) -> Result<Self> {
        // For tests use the standard default interval of 1000ms. Tests that need a faster
        // interval should call `new_for_test_with_interval` below.
        let task_supervisor = TaskSupervisor::new_with_worker_count_tx(
            config.collector_uuid.clone(),
            1000,
            cached_intent,
            None,
        );
        Ok(Self {
            config,
            task_supervisor: Some(task_supervisor),
            _lock: lock,
            supervisor_handle: None,
            supervisor_shutdown_tx: None,
            test_shutdown_tx_sender,
            client_holder: None,
            cache_file_path: None, // No caching in tests
        })
    }

    /// Test helper which allows specifying a custom health interval (ms).
    pub fn new_for_test_with_interval(
        config: Config,
        lock: TcpListener,
        test_shutdown_tx_sender: Option<mpsc::Sender<mpsc::Sender<SupervisorShutdown>>>,
        health_interval_ms: u64,
        cached_intent: Option<CachedIntent>,
    ) -> Result<Self> {
        let task_supervisor = TaskSupervisor::new_with_worker_count_tx(
            config.collector_uuid.clone(),
            health_interval_ms,
            cached_intent,
            None,
        );
        Ok(Self {
            config,
            task_supervisor: Some(task_supervisor),
            _lock: lock,
            supervisor_handle: None,
            supervisor_shutdown_tx: None,
            test_shutdown_tx_sender,
            client_holder: None,
            cache_file_path: None, // No caching in tests
        })
    }

    /// Test helper which allows specifying a custom health interval (ms) and a
    /// test-only worker_count_tx to observe reconciliation worker counts.
    pub fn new_for_test_with_interval_and_worker_tx(
        config: Config,
        lock: TcpListener,
        test_shutdown_tx_sender: Option<mpsc::Sender<mpsc::Sender<SupervisorShutdown>>>,
        health_interval_ms: u64,
        cached_intent: Option<CachedIntent>,
        worker_count_tx: Option<mpsc::Sender<usize>>,
    ) -> Result<Self> {
        let task_supervisor = TaskSupervisor::new_with_worker_count_tx(
            config.collector_uuid.clone(),
            health_interval_ms,
            cached_intent,
            worker_count_tx,
        );
        Ok(Self {
            config,
            task_supervisor: Some(task_supervisor),
            _lock: lock,
            supervisor_handle: None,
            supervisor_shutdown_tx: None,
            test_shutdown_tx_sender,
            client_holder: None,
            cache_file_path: None, // No caching in tests
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
                warn!("TaskSupervisor exited with an error: {e}");
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
        let (client_tx, mut client_rx) = mpsc::channel::<Arc<dyn DatabaseClientTrait>>(1);
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
        // Channel to receive fsync notifications from the SessionHandler
        let (fsync_tx, fsync_rx) = mpsc::channel::<u64>(10);

        self.supervisor_shutdown_tx = Some(supervisor_shutdown_tx.clone());
        if let Some(sender) = &self.test_shutdown_tx_sender
            && sender.send(supervisor_shutdown_tx).await.is_err()
        {
            warn!("Failed to send supervisor_shutdown_tx to test harness. Test might hang.");
        }
        let reconnect_notify = Arc::new(Notify::new());

        // Create a shared, updatable ClientHolder and keep a copy on self so
        // tests or other components can access the current client if needed.
        let client_holder = ClientHolder::new(None);
        self.client_holder = Some(client_holder.clone());

        // Persistence task
        let cache_file_path = self.cache_file_path.clone();
        tokio::spawn(async move {
            while let Some(intent) = persistence_rx.recv().await {
                if let Some(ref path) = cache_file_path
                    && let Ok(ron_string) = ron::to_string(&intent)
                    && let Err(e) = tokio::fs::write(path, ron_string).await
                {
                    warn!("Failed to write {path}: {e}");
                }
            }
        });

        // ConnectionManager task
        let connection_manager = ConnectionManager::new(
            self.config.database_addr.clone(),
            self.config.auth_token.clone(),
            client_tx,
            reconnect_notify.clone(),
            Some(client_holder.clone()),
        );
        tokio::spawn(connection_manager.run());

        // TaskSupervisor task
        let supervisor = self.task_supervisor.take().expect("No supervisor");
        self.supervisor_handle = Some(tokio::spawn(supervisor.run(
            config_rx,
            client_update_rx,
            health_report_tx,
            supervisor_shutdown_rx,
            fsync_rx,
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
                        client_update_tx.send(ClientUpdate::NewClient(client.clone())).await.ok();
                        let session_handler = SessionHandler::new(
                            client.clone(),
                            config_tx.clone(),
                            self.config.collector_uuid.clone(),
                            latest_health_rx.clone(),
                            persistence_tx.clone(),
                            fsync_tx.clone(),
                            self.config.use_mock_ping_client,
                            self.cache_file_path.as_ref().map(|s| s.as_str().into()),
                        );
                        let session_handle = tokio::spawn(session_handler.run());

                        if let Err(e) = session_handle.await {
                            warn!("Session ended with an error: {e:?}");
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
                    Ok(Err(e)) => warn!("TaskSupervisor exited with an error: {e}"),
                    Err(e) => warn!("TaskSupervisor task panicked: {e}"),
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
            use_mock_ping_client: true,
        }
    }

    #[tokio::test]
    #[timeout(100)]
    async fn test_collector_service_new() {
        let config = mock_config();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let service = CollectorService::new(config, listener, None);
        assert!(service.is_ok());
    }
}
