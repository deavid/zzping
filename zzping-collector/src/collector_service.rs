use crate::{
    config::Config,
    connection_manager::ConnectionManager,
    database_client::DatabaseClient,
    session_handler::SessionHandler,
    task_supervisor::{ClientUpdate, HealthReport, SupervisorConfig, TaskSupervisor},
};
use anyhow::Result;
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::net::{IpAddr, TcpListener};
use std::sync::Arc;
use tokio::sync::{mpsc, watch, Notify};
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
    task_supervisor: TaskSupervisor,
    _lock: TcpListener,
}

impl CollectorService {
    /// Creates a new `CollectorService`.
    pub fn new(config: Config, lock: TcpListener) -> Result<Self> {
        let task_supervisor = TaskSupervisor::new(config.collector_uuid.clone());
        Ok(Self {
            config,
            task_supervisor,
            _lock: lock,
        })
    }

    /// Runs the `CollectorService` to completion.
    pub async fn run(self) -> Result<()> {
        info!("CollectorService running.");

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

        let reconnect_notify = Arc::new(Notify::new());

        // This task handles writing the last known intent to disk.
        tokio::spawn(async move {
            while let Some(intent) = persistence_rx.recv().await {
                match ron::to_string(&intent) {
                    Ok(ron_string) => {
                        if let Err(e) = tokio::fs::write("last_intent.ron", ron_string).await {
                            warn!("Failed to write last_intent.ron: {e}");
                        } else {
                            info!("Successfully persisted last known intent to last_intent.ron");
                        }
                    }
                    Err(e) => {
                        warn!("Failed to serialize CachedIntent to RON: {e}");
                    }
                }
            }
        });

        let connection_manager = ConnectionManager::new(
            self.config.database_addr.clone(),
            self.config.auth_token.clone(),
            client_tx,
            reconnect_notify.clone(),
        );
        tokio::spawn(connection_manager.run());

        let supervisor_handle =
            tokio::spawn(
                self.task_supervisor
                    .run(config_rx, client_update_rx, health_report_tx),
            );

        // This task forwards health reports from the mpsc channel to the watch channel.
        let health_forwarder_handle = tokio::spawn(async move {
            while let Some(report) = health_report_rx.recv().await {
                debug!("Forwarding new health report: {report:?}");
                latest_health_tx.send(report).ok();
            }
        });

        info!("Waiting for a database connection...");
        while let Some(client) = client_rx.recv().await {
            info!("Received new database client. Spawning SessionHandler.");
            client_update_tx
                .send(ClientUpdate::NewClient(Box::new(client.clone())))
                .await?;

            let session_handler = SessionHandler::new(
                client,
                config_tx.clone(),
                self.config.collector_uuid.clone(),
                latest_health_rx.clone(),
                persistence_tx.clone(),
            );
            let session_handle = tokio::spawn(session_handler.run());

            session_handle.await??;

            warn!("Session ended. Notifying supervisor and requesting new connection.");
            client_update_tx.send(ClientUpdate::ClientLost).await?;
            reconnect_notify.notify_one();
        }

        supervisor_handle.await??;
        health_forwarder_handle.abort();
        info!("CollectorService has shut down.");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_config() -> Config {
        Config {
            collector_uuid: "test-uuid".to_string(),
            database_addr: "http://127.0.0.1:0".to_string(),
            auth_token: "test-token".to_string(),
        }
    }

    #[tokio::test]
    async fn test_collector_service_new() {
        let config = mock_config();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let service = CollectorService::new(config, listener);
        assert!(service.is_ok());
    }
}
