use crate::{
    config::Config,
    connection_manager::ConnectionManager,
    database_client::DatabaseClient,
    session_handler::SessionHandler,
    task_supervisor::{SupervisorConfig, TaskSupervisor},
};
use anyhow::Result;
use log::info;
use tokio::sync::{mpsc, watch};

/// The long-lived root of the application, owning all resilient states
/// (via its child components) and supervising the overall connection lifecycle.
pub struct CollectorService {
    config: Config,
    task_supervisor: TaskSupervisor,
}

impl CollectorService {
    /// Creates a new `CollectorService`.
    pub fn new(config: Config) -> Result<Self> {
        let task_supervisor = TaskSupervisor::new()?;
        Ok(Self {
            config,
            task_supervisor,
        })
    }

    /// Runs the `CollectorService` to completion.
    pub async fn run(self) -> Result<()> {
        info!("CollectorService running.");

        // Create the channels for communication between components.
        let (client_tx, mut client_rx) = mpsc::channel::<DatabaseClient>(1);
        let (config_tx, config_rx) = watch::channel::<Option<SupervisorConfig>>(None);

        // Spawn the permanent ConnectionManager task.
        let connection_manager = ConnectionManager::new(
            self.config.database_addr.clone(),
            self.config.auth_token.clone(),
            client_tx,
        );
        tokio::spawn(connection_manager.run());

        // Spawn the permanent TaskSupervisor task.
        // We pass it the receiver end of the config channel.
        let supervisor_handle = tokio::spawn(self.task_supervisor.run(config_rx));

        info!("Waiting for a database connection...");
        // Main loop: Supervise sessions.
        while let Some(client) = client_rx.recv().await {
            info!("Received new database client. Spawning SessionHandler.");

            // For each new connection, spawn an ephemeral SessionHandler.
            let session_handler = SessionHandler::new(
                client,
                config_tx.clone(),
                self.config.collector_uuid.clone(),
            );
            tokio::spawn(session_handler.run());
        }

        // If the client_rx loop exits, it means the ConnectionManager has shut down.
        // We can now wait for the supervisor to finish.
        supervisor_handle.await??;

        info!("CollectorService has shut down.");
        Ok(())
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    // We need a mock config for testing.
    fn mock_config() -> Config {
        Config {
            collector_uuid: "test-uuid".to_string(),
            database_addr: "http://127.0.0.1:0".to_string(), // Invalid port
            auth_token: "test-token".to_string(),
        }
    }

    #[tokio::test]
    async fn test_collector_service_new() {
        let config = mock_config();
        let service = CollectorService::new(config);
        assert!(service.is_ok());
    }

    // The full test for the service's `run` method will be the
    // new integration test, as it requires mocking multiple components
    // and their interactions.
}
