use anyhow::Result;
use log::info;
use tokio::sync::watch;

// This is a placeholder for the actual config. It will be defined properly in a later step.
#[derive(Debug, Clone, PartialEq)]
pub struct SupervisorConfig {
    pub placeholder: String,
}

/// The long-lived manager of the worker pool, responsible for creating,
/// destroying, and commanding workers to match the latest configuration
/// received from the database.
pub struct TaskSupervisor {}

impl TaskSupervisor {
    /// Creates a new `TaskSupervisor`.
    pub fn new() -> Result<Self> {
        Ok(Self {})
    }

    /// Runs the `TaskSupervisor`'s reconciliation loop.
    pub async fn run(
        self,
        mut config_rx: watch::Receiver<Option<SupervisorConfig>>,
    ) -> Result<()> {
        info!("TaskSupervisor running.");
        loop {
            // Wait for a new configuration to be received.
            if config_rx.changed().await.is_err() {
                // The channel was closed, which means the sender (SessionHandler) was dropped.
                // This is a signal to shut down.
                info!("Configuration channel closed. TaskSupervisor shutting down.");
                break;
            }

            let config = config_rx.borrow().clone();

            if let Some(config) = config {
                // In the future, this is where the reconciliation logic will go.
                // For now, we just log what we would do.
                info!(
                    "TaskSupervisor received new config. Would apply: {:?}",
                    config
                );
            } else {
                info!("TaskSupervisor received empty config. Would remove all workers.");
            }
        }
        Ok(())
    }
}

impl Default for TaskSupervisor {
    fn default() -> Self {
        Self::new().unwrap()
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::time::timeout;

    #[tokio::test]
    async fn test_task_supervisor_new() {
        let supervisor = TaskSupervisor::new();
        assert!(supervisor.is_ok());
    }

    #[tokio::test]
    async fn test_task_supervisor_receives_config_and_logs() {
        let (config_tx, config_rx) = watch::channel(None);
        let supervisor = TaskSupervisor::new().unwrap();

        let supervisor_handle = tokio::spawn(supervisor.run(config_rx));

        // Send a new config
        let new_config = SupervisorConfig {
            placeholder: "test_config".to_string(),
        };
        config_tx.send(Some(new_config.clone())).unwrap();

        // Give the supervisor a moment to process
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Check that the last sent value is correct
        assert_eq!(*config_tx.borrow(), Some(new_config));

        // Test shutdown
        drop(config_tx);

        // The supervisor should shut down gracefully.
        let result = timeout(Duration::from_secs(1), supervisor_handle).await;
        assert!(result.is_ok(), "Supervisor did not shut down gracefully");
    }
}
