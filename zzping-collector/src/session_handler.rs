use crate::{
    collector_service::CachedIntent,
    database_client::DatabaseClient,
    task_supervisor::{HealthReport, SupervisorConfig},
};
use anyhow::Result;
use log::{error, info, warn};
use std::{collections::HashSet, time::Duration};
use tokio::sync::{mpsc, watch};
use zzping_proto::zzping::{
    CollectorRole, Command, CommandRequest, HeartbeatRequest, HeartbeatResponse,
    command::CommandType,
};

/// An internal message to the state manager loop.
enum SessionUpdate {
    FromHeartbeat(HeartbeatResponse),
    FromCommand(Command),
}

/// An ephemeral task that manages all gRPC communication for the duration of
/// a single, healthy connection. It dies gracefully on any network error.
pub struct SessionHandler {
    /// The gRPC client for this session.
    client: DatabaseClient,
    /// The sender for broadcasting configuration updates.
    config_tx: watch::Sender<Option<SupervisorConfig>>,
    /// The UUID of this collector.
    collector_uuid: String,
    /// A receiver for the latest health report from the service.
    health_rx: watch::Receiver<HealthReport>,
    /// A sender for persisting the latest config.
    persistence_tx: mpsc::Sender<CachedIntent>,
}

impl SessionHandler {
    /// Creates a new `SessionHandler`.
    pub fn new(
        client: DatabaseClient,
        config_tx: watch::Sender<Option<SupervisorConfig>>,
        collector_uuid: String,
        health_rx: watch::Receiver<HealthReport>,
        persistence_tx: mpsc::Sender<CachedIntent>,
    ) -> Self {
        Self {
            client,
            config_tx,
            collector_uuid,
            health_rx,
            persistence_tx,
        }
    }

    /// Runs the `SessionHandler`'s main loops.
    pub async fn run(self) -> Result<()> {
        info!("SessionHandler started.");

        let (update_tx, update_rx) = mpsc::channel(10);
        let client_clone1 = self.client.clone();
        let client_clone2 = self.client.clone();

        let heartbeat_handle = tokio::spawn(Self::run_heartbeat_loop(
            client_clone1,
            self.health_rx.clone(),
            self.collector_uuid.clone(),
            update_tx.clone(),
        ));
        let command_handle = tokio::spawn(Self::run_command_loop(
            client_clone2,
            self.collector_uuid,
            update_tx,
        ));
        let state_manager_handle = tokio::spawn(Self::run_state_manager_loop(
            update_rx,
            self.config_tx,
            self.persistence_tx,
        ));

        // The session ends if any of the core loops fails.
        tokio::select! {
            res = heartbeat_handle => { warn!("Heartbeat loop ended."); res?? }
            res = command_handle => { warn!("Command stream loop ended."); res?? }
            res = state_manager_handle => { warn!("State manager loop ended."); res?? }
        }

        Ok(())
    }

    async fn run_state_manager_loop(
        mut update_rx: mpsc::Receiver<SessionUpdate>,
        config_tx: watch::Sender<Option<SupervisorConfig>>,
        persistence_tx: mpsc::Sender<CachedIntent>,
    ) -> Result<()> {
        let mut current_config = SupervisorConfig {
            targets: HashSet::new(),
            ping_rate_pps: 0,
            role: CollectorRole::Standby,
        };
        // TODO: We should probably load the last known config from disk here.

        while let Some(update) = update_rx.recv().await {
            match update {
                SessionUpdate::FromHeartbeat(response) => {
                    current_config.role =
                        CollectorRole::try_from(response.role).unwrap_or(CollectorRole::Standby);
                    current_config.targets = response
                        .targets
                        .into_iter()
                        .filter_map(|s| s.parse().ok())
                        .collect();
                    current_config.ping_rate_pps = response.ping_rate_pps;
                }
                SessionUpdate::FromCommand(command) => {
                    if let Some(command_type) = command.command_type {
                        match command_type {
                            CommandType::ChangeRole(role) => {
                                current_config.role =
                                    CollectorRole::try_from(role).unwrap_or(current_config.role);
                            }
                            CommandType::PrepareToSwap(_) => {
                                // TODO: Handle this command
                                warn!("PrepareToSwap command not yet implemented.");
                            }
                        }
                    }
                }
            }
            info!("Broadcasting new config: {current_config:?}");
            if config_tx.send(Some(current_config.clone())).is_err() {
                info!("Config channel closed, state manager shutting down.");
                break;
            }

            // Also send the config to be persisted.
            let intent = CachedIntent {
                targets: current_config.targets.clone(),
                ping_rate_pps: current_config.ping_rate_pps,
            };
            if persistence_tx.send(intent).await.is_err() {
                info!("Persistence channel closed, state manager shutting down.");
                break;
            }
        }
        Ok(())
    }

    async fn run_heartbeat_loop(
        mut client: DatabaseClient,
        health_rx: watch::Receiver<HealthReport>,
        collector_uuid: String,
        update_tx: mpsc::Sender<SessionUpdate>,
    ) -> Result<()> {
        // FIXME: This interval must be configurable externally, specially for unit tests!
        let mut interval = tokio::time::interval(if cfg!(test) {
            Duration::from_millis(1)
        } else {
            Duration::from_millis(100)
        });
        let last_processed_command_id = 0; // Will be updated later

        loop {
            interval.tick().await;
            let health_report = health_rx.borrow().clone();

            let request = HeartbeatRequest {
                collector_uuid: collector_uuid.clone(),
                pid: std::process::id() as u64,
                current_role: health_report.role.into(),
                buffer_record_count: health_report.total_buffer_size as u64,
                last_fatal_error: health_report.fatal_errors.join(", "),
                last_processed_command_id,
            };

            // Add timeout to heartbeat request
            let heartbeat_result = tokio::time::timeout(
                Duration::from_millis(500), // Timeout for heartbeat
                client.heartbeat(request),
            )
            .await;

            match heartbeat_result {
                Ok(result) => match result {
                    Ok(response) => {
                        if update_tx
                            .send(SessionUpdate::FromHeartbeat(response.into_inner()))
                            .await
                            .is_err()
                        {
                            info!("State manager disconnected, heartbeat loop shutting down.");
                            break;
                        }
                    }
                    Err(e) => {
                        error!("Heartbeat RPC failed: {e}. Session ending.");
                        return Err(e);
                    }
                },
                Err(_) => {
                    error!("Heartbeat request timed out - connection likely failed.");
                    return Err(anyhow::anyhow!("Heartbeat timeout"));
                }
            }
        }
        Ok(())
    }

    async fn run_command_loop(
        mut client: DatabaseClient,
        collector_uuid: String,
        update_tx: mpsc::Sender<SessionUpdate>,
    ) -> Result<()> {
        let request = CommandRequest { collector_uuid };
        let mut stream = client.subscribe_to_commands(request).await?.into_inner();
        info!("Successfully subscribed to command stream.");

        loop {
            // Add timeout to detect if stream is stuck
            let message_result = tokio::time::timeout(
                if cfg!(test) {
                    // Short timeout in tests
                    Duration::from_millis(10)
                } else {
                    Duration::from_millis(1000)
                },
                stream.message(),
            )
            .await;

            match message_result {
                Ok(message_result) => match message_result? {
                    Some(command) => {
                        info!("Received command: {command:?}");
                        if update_tx
                            .send(SessionUpdate::FromCommand(command))
                            .await
                            .is_err()
                        {
                            info!("State manager disconnected, command loop shutting down.");
                            break;
                        }
                    }
                    None => {
                        info!("Command stream ended (received None).");
                        break;
                    }
                },
                Err(_) => {
                    // Timeout occurred - this might indicate connection issues
                    info!("Command stream message timeout - checking if we should exit.");

                    // Check if the update_tx is still alive (state manager still running)
                    if update_tx.is_closed() {
                        info!("Update channel closed, command loop shutting down.");
                        break;
                    }

                    // Continue the loop to try again
                    continue;
                }
            }
        }

        warn!("Command stream from database was closed.");
        Ok(())
    }
}
