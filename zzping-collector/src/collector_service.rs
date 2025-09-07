use crate::{
    config::Config,
    connection_manager::ConnectionManager,
    database_client::DatabaseClient,
    session_handler::SessionHandler,
    task_supervisor::{ClientUpdate, SupervisorConfig, TaskSupervisor},
};
use anyhow::Result;
use log::{info, warn};
use std::net::TcpListener;
use tokio::sync::{mpsc, watch};
use std::sync::Arc;
use tokio::sync::Notify;

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
        let reconnect_notify = Arc::new(Notify::new());

        let connection_manager = ConnectionManager::new(
            self.config.database_addr.clone(),
            self.config.auth_token.clone(),
            client_tx,
            reconnect_notify.clone(),
        );
        tokio::spawn(connection_manager.run());

        let supervisor_handle =
            tokio::spawn(self.task_supervisor.run(config_rx, client_update_rx));

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
            );
            let session_handle = tokio::spawn(session_handler.run());

            session_handle.await??;

            warn!("Session ended. Notifying supervisor and requesting new connection.");
            client_update_tx.send(ClientUpdate::ClientLost).await?;
            reconnect_notify.notify_one();
        }

        supervisor_handle.await??;
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
