use crate::database_client::DatabaseClient;
use crate::task_supervisor::SupervisorConfig;
use anyhow::Result;
use log::{error, info};
use std::time::Duration;
use tokio::sync::watch;
use zzping_proto::zzping::{CollectorRole, HeartbeatRequest};

/// An ephemeral task that manages all gRPC communication for the duration of
/// a single, healthy connection. It dies gracefully on any network error.
pub struct SessionHandler {
    /// The gRPC client for this session.
    client: DatabaseClient,
    /// The sender for broadcasting configuration updates.
    config_tx: watch::Sender<Option<SupervisorConfig>>,
    /// The UUID of this collector.
    collector_uuid: String,
}

impl SessionHandler {
    /// Creates a new `SessionHandler`.
    pub fn new(
        client: DatabaseClient,
        config_tx: watch::Sender<Option<SupervisorConfig>>,
        collector_uuid: String,
    ) -> Self {
        Self {
            client,
            config_tx,
            collector_uuid,
        }
    }

    /// Runs the `SessionHandler`'s main loop.
    pub async fn run(mut self) -> Result<()> {
        info!("SessionHandler started.");
        let mut interval = tokio::time::interval(Duration::from_secs(1));

        loop {
            interval.tick().await;

            let request = HeartbeatRequest {
                collector_uuid: self.collector_uuid.clone(),
                pid: std::process::id() as u64,
            };

            match self.client.heartbeat(request).await {
                Ok(response) => {
                    let response = response.into_inner();
                    // This conversion will be more complex later.
                    let _role = CollectorRole::try_from(response.role)
                        .unwrap_or(CollectorRole::Standby);

                    let targets = response
                        .targets
                        .into_iter()
                        .filter_map(|s| s.parse::<std::net::IpAddr>().ok())
                        .collect();

                    let config = SupervisorConfig {
                        targets,
                        ping_rate_pps: response.ping_rate_pps,
                    };

                    if self.config_tx.send(Some(config)).is_err() {
                        // The receiver was dropped, so we can shut down.
                        info!("Config channel closed. SessionHandler shutting down.");
                        break;
                    }
                }
                Err(e) => {
                    error!("Heartbeat RPC failed: {e}. Session ending.");
                    // Any gRPC error is considered fatal for the session.
                    // The task will terminate, signaling the CollectorService
                    // to request a new connection.
                    break;
                }
            }
        }
        Ok(())
    }
}

