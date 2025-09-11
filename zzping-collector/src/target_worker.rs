use crate::{
    batch_submitter::{BatchSubmitter, BatchSubmitterCommand},
    database_client::DatabaseClientTrait,
    ping_client::PingClient,
    ping_surge_client::PingSurgeClient,
    pinger::{FinalizedPing, Pinger, PingerCommand},
};
use anyhow::Result;
use log::{debug, info, warn};
use std::{net::IpAddr, sync::Arc, time::Duration};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use zzping_proto::zzping::CollectorRole;

/// The health status of a worker, reported to the supervisor.
#[derive(Debug)]
pub struct WorkerHealth {
    pub buffer_size: usize,
}

// A command enum for the TargetWorker.
#[derive(Debug)]
pub enum WorkerCommand {
    GetHealth(oneshot::Sender<WorkerHealth>),
    UpdateRole(CollectorRole),
    PruneByFsync(u64),
    Shutdown,
}

/// A handle to a running TargetWorker, allowing the TaskSupervisor to command it.
#[derive(Debug)]
pub struct TargetWorkerHandle {
    pub command_tx: mpsc::Sender<WorkerCommand>,
    pub task_handle: JoinHandle<()>,
}

/// A collection of handles for a TargetWorker, including a channel to inject data for testing.
#[derive(Debug)]
pub struct TargetWorkerHandles {
    pub handle: TargetWorkerHandle,
    pub data_tx: mpsc::Sender<FinalizedPing>,
}

use zzping_proto::zzping::GetRecentDataRequest;

/// The TargetWorker is a self-contained, independent task that owns all state
/// and logic for monitoring a single target IP address.
pub struct TargetWorker {
    target_ip: IpAddr,
    db_client: Arc<dyn DatabaseClientTrait>,
    pinger: Pinger,
    pinger_command_tx: mpsc::Sender<PingerCommand>,
    batch_submitter_command_tx: mpsc::Sender<BatchSubmitterCommand>,
    batch_submitter: BatchSubmitter,
    command_rx: mpsc::Receiver<WorkerCommand>,
}

impl TargetWorker {
    /// Creates a new TargetWorker and a handle to communicate with it.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        collector_uuid: String,
        target_ip: IpAddr,
        ping_rate_pps: u64,
        db_client: Arc<dyn DatabaseClientTrait>,
    ) -> Result<TargetWorkerHandles> {
        let ping_client = Arc::new(PingSurgeClient::new(target_ip)?);
        Self::new_with_ping_client(
            collector_uuid,
            target_ip,
            ping_rate_pps,
            db_client,
            ping_client,
        )
    }

    /// Creates a new TargetWorker with a custom PingClient (useful for testing).
    #[allow(clippy::new_ret_no_self)]
    pub fn new_with_ping_client(
        collector_uuid: String,
        target_ip: IpAddr,
        ping_rate_pps: u64,
        db_client: Arc<dyn DatabaseClientTrait>,
        ping_client: Arc<dyn PingClient>,
    ) -> Result<TargetWorkerHandles> {
        let (command_tx, command_rx) = mpsc::channel(10);
        let (results_tx, results_rx) = mpsc::channel::<FinalizedPing>(100);
        let (pinger_command_tx, pinger_command_rx) = mpsc::channel(10);
        let (batch_submitter_command_tx, batch_submitter_command_rx) = mpsc::channel(10);

        let pinger = Pinger::new(
            target_ip,
            ping_rate_pps,
            Duration::from_secs(60),
            Duration::from_secs(5),
            Duration::from_secs(60),
            ping_client,
            results_tx.clone(),
            db_client.clone(),
            pinger_command_rx,
        );

        let batch_submitter = BatchSubmitter::new(
            collector_uuid,
            target_ip,
            Duration::from_secs(60),
            1_000_000,
            Duration::from_secs(24 * 3600),
            db_client.clone(),
            batch_submitter_command_rx,
        );

        let worker = Self {
            target_ip,
            db_client,
            pinger,
            pinger_command_tx,
            batch_submitter_command_tx,
            batch_submitter,
            command_rx,
        };

        let task_handle = tokio::spawn(async move {
            worker.run(results_rx).await;
        });

        Ok(TargetWorkerHandles {
            handle: TargetWorkerHandle {
                command_tx,
                task_handle,
            },
            data_tx: results_tx,
        })
    }

    /// Runs the TargetWorker's main loop, which spawns and supervises the
    /// Pinger and BatchSubmitter tasks.
    async fn run(self, results_rx: mpsc::Receiver<FinalizedPing>) {
        info!("TargetWorker started for target {}", self.target_ip);

        // Move components out of self to avoid partial move errors.
        let pinger = self.pinger;
        let pinger_command_tx = self.pinger_command_tx;
        let batch_submitter_command_tx = self.batch_submitter_command_tx;
        let batch_submitter = self.batch_submitter;
        let mut command_rx = self.command_rx;

        // Spawn children tasks and store their handles for proper shutdown coordination
        let mut pinger_handle = tokio::spawn(pinger.run());
        let mut submitter_handle = tokio::spawn(batch_submitter.run(results_rx));

        // Track whether we should perform an ordered shutdown sequence after the loop.
        let mut perform_ordered_shutdown = false;

        loop {
            tokio::select! {
                Some(command) = command_rx.recv() => {
                    match command {
                        WorkerCommand::GetHealth(tx) => {
                            debug!("TargetWorker[{}] received GetHealth request", self.target_ip);
                            let (health_tx, health_rx) = oneshot::channel();
                            if batch_submitter_command_tx.send(BatchSubmitterCommand::GetHealth(health_tx)).await.is_err() {
                                warn!("Failed to send GetHealth command to batch_submitter for target {}.", self.target_ip);
                                let _ = tx.send(WorkerHealth { buffer_size: 0 });
                            } else {
                                match health_rx.await {
                                    Ok(buffer_size) => {
                                        info!(
                                            "TargetWorker for {} reporting health: buffer_size = {}",
                                            self.target_ip, buffer_size
                                        );
                                        let health = WorkerHealth { buffer_size };
                                        let _ = tx.send(health);
                                    }
                                    Err(_) => {
                                        warn!("Failed to receive health response from batch_submitter for target {}.", self.target_ip);
                                        let _ = tx.send(WorkerHealth { buffer_size: 0 });
                                    }
                                }
                            }
                        }
                        WorkerCommand::UpdateRole(role) => {
                            info!("TargetWorker for {} updating role to {:?}", self.target_ip, role);

                            // If we are becoming PRIMARY_SUPERVISED, we need to get the recent data
                            // and initialize the batch submitter's ACK cursor synchronously.
                            if role == CollectorRole::PrimarySupervised {
                                info!("TargetWorker becoming PRIMARY_SUPERVISED, fetching recent data.");
                                let request = GetRecentDataRequest {
                                    // These fields are not used yet, but will be.
                                    collector_uuid: "".to_string(),
                                    lookback_seconds: 30,
                                };
                                let db_client = self.db_client.clone();
                                match db_client.get_recent_data(request).await {
                                    Ok(response) => {
                                        let ack_nanos = response.into_inner().database_confirms_last_acked_received_nanos;
                                        info!("Handoff: Initializing ACK cursor to {} for target {}", ack_nanos, self.target_ip);
                                        if batch_submitter_command_tx.send(BatchSubmitterCommand::InitializeAckCursor(ack_nanos)).await.is_err() {
                                            warn!("Failed to send InitializeAckCursor command to BatchSubmitter for target {}.", self.target_ip);
                                        } else {
                                            info!("Handoff: ACK cursor initialization completed for target {}", self.target_ip);
                                        }
                                    }
                                    Err(e) => {
                                        warn!("Failed to get recent data for target {}: {}", self.target_ip, e);
                                    }
                                }
                            }

                            if pinger_command_tx.send(PingerCommand::UpdateRole(role)).await.is_err() {
                                warn!("Failed to send UpdateRole command to pinger for target {}.", self.target_ip);
                            }
                            if batch_submitter_command_tx.send(BatchSubmitterCommand::UpdateRole(role)).await.is_err() {
                                warn!("Failed to send UpdateRole command to batch_submitter for target {}.", self.target_ip);
                            }
                        }
                        WorkerCommand::PruneByFsync(fsync_nanos) => {
                            debug!("TargetWorker[{}] received PruneByFsync({})", self.target_ip, fsync_nanos);
                            info!("TargetWorker for {} received PruneByFsync {}.", self.target_ip, fsync_nanos);
                            if batch_submitter_command_tx.send(BatchSubmitterCommand::PruneByFsync(fsync_nanos)).await.is_err() {
                                warn!("Failed to forward PruneByFsync to BatchSubmitter for target {}.", self.target_ip);
                            }
                        }
                        WorkerCommand::Shutdown => {
                            info!(
                                "SHUTDOWN_LOG: TargetWorker for {} received shutdown command.",
                                self.target_ip
                            );
                            // Request ordered shutdown: first stop the pinger so no new
                            // data is generated.
                            if pinger_command_tx.send(PingerCommand::Shutdown).await.is_err() {
                                warn!("Failed to send Shutdown to pinger for {}.", self.target_ip);
                            }
                            perform_ordered_shutdown = true;
                            info!("SHUTDOWN_LOG: TargetWorker for {} requested ordered shutdown.", self.target_ip);
                            break; // Exit the loop to perform ordered shutdown below
                        }
                    }
                }
                // Exit if a child task finishes unexpectedly
                result = &mut pinger_handle => {
                    match result {
                        Ok(_) => warn!("Pinger task for {} exited.", self.target_ip),
                        Err(e) => warn!("Pinger task for {} panicked: {}", self.target_ip, e),
                    }
                    break;
                }
                result = &mut submitter_handle => {
                    match result {
                        Ok(_) => warn!("BatchSubmitter task for {} exited.", self.target_ip),
                        Err(e) => warn!("BatchSubmitter task for {} panicked: {}", self.target_ip, e),
                    }
                    break;
                }
                else => {
                    info!("TargetWorker for {} channels closed. Shutting down.", self.target_ip);
                    break;
                }
            }
        }

        // After the loop, await the child tasks to ensure they shut down cleanly.
        info!(
            "SHUTDOWN_LOG: TargetWorker for {} waiting for child tasks to complete.",
            self.target_ip
        );
        if perform_ordered_shutdown {
            // We already requested the pinger to shut down before exiting the loop.
            // Now wait for it to complete.
            info!(
                "SHUTDOWN_LOG: TargetWorker for {} performing ordered shutdown.",
                self.target_ip
            );

            match pinger_handle.await {
                Ok(_) => info!("SHUTDOWN_LOG: Pinger for {} finished.", self.target_ip),
                Err(e) => warn!(
                    "Pinger task for {} panicked during shutdown: {}",
                    self.target_ip, e
                ),
            }

            // Now tell the batch submitter to shut down and wait for it to finish.
            if batch_submitter_command_tx
                .send(BatchSubmitterCommand::Shutdown)
                .await
                .is_err()
            {
                warn!(
                    "Failed to send Shutdown to batch_submitter for {}.",
                    self.target_ip
                );
            }

            match submitter_handle.await {
                Ok(_) => info!(
                    "SHUTDOWN_LOG: BatchSubmitter for {} finished.",
                    self.target_ip
                ),
                Err(e) => warn!(
                    "BatchSubmitter task for {} panicked during shutdown: {}",
                    self.target_ip, e
                ),
            }
        } else {
            // If we didn't request ordered shutdown, just wait for both children
            // to finish (they might have exited unexpectedly already).
            let (pinger_result, submitter_result) = tokio::join!(pinger_handle, submitter_handle);
            match pinger_result {
                Ok(_) => info!("SHUTDOWN_LOG: Pinger for {} joined.", self.target_ip),
                Err(e) => warn!("Pinger task for {} panicked: {}", self.target_ip, e),
            }
            match submitter_result {
                Ok(_) => info!("SHUTDOWN_LOG: Submitter for {} joined.", self.target_ip),
                Err(e) => warn!("Submitter task for {} panicked: {}", self.target_ip, e),
            }
        }
        info!("TargetWorker for {} has shut down.", self.target_ip);
    }
}
