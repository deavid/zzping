use crate::{
    batch_submitter::{BatchSubmitter, SharedBuffer},
    database_client::DatabaseClient,
    pinger::{FinalizedPing, Pinger, PingerCommand},
    ping_surge_client::PingSurgeClient,
};
use anyhow::Result;
use log::{info, warn};
use std::{
    collections::BTreeMap,
    net::IpAddr,
    sync::{Arc, Mutex},
    time::Duration,
};
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
    Shutdown,
}

/// A handle to a running TargetWorker, allowing the TaskSupervisor to command it.
#[derive(Debug)]
pub struct TargetWorkerHandle {
    pub command_tx: mpsc::Sender<WorkerCommand>,
    pub task_handle: JoinHandle<()>,
}

/// The TargetWorker is a self-contained, independent task that owns all state
/// and logic for monitoring a single target IP address.
pub struct TargetWorker {
    target_ip: IpAddr,
    pinger: Pinger,
    pinger_command_tx: mpsc::Sender<PingerCommand>,
    batch_submitter: BatchSubmitter,
    command_rx: mpsc::Receiver<WorkerCommand>,
    buffer: SharedBuffer,
}

impl TargetWorker {
    /// Creates a new TargetWorker and a handle to communicate with it.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        collector_uuid: String,
        target_ip: IpAddr,
        ping_rate_pps: u64,
        db_client: DatabaseClient,
    ) -> Result<TargetWorkerHandle> {
        let (command_tx, command_rx) = mpsc::channel(10);
        let (results_tx, results_rx) = mpsc::channel::<FinalizedPing>(100);
        let (pinger_command_tx, pinger_command_rx) = mpsc::channel(10);

        let ping_client = Arc::new(PingSurgeClient::new(target_ip)?);

        let pinger = Pinger::new(
            target_ip,
            ping_rate_pps,
            Duration::from_secs(60),
            Duration::from_secs(5),
            ping_client,
            results_tx,
            db_client.clone(),
            pinger_command_rx,
        );

        let buffer = Arc::new(Mutex::new(BTreeMap::new()));

        let batch_submitter = BatchSubmitter::new(
            collector_uuid,
            target_ip,
            Duration::from_secs(60),
            1_000_000,
            Duration::from_secs(24 * 3600),
            db_client,
            buffer.clone(),
        );

        let worker = Self {
            target_ip,
            pinger,
            pinger_command_tx,
            batch_submitter,
            command_rx,
            buffer,
        };

        let task_handle = tokio::spawn(async move {
            worker.run(results_rx).await;
        });

        Ok(TargetWorkerHandle {
            command_tx,
            task_handle,
        })
    }

    /// Runs the TargetWorker's main loop, which spawns and supervises the
    /// Pinger and BatchSubmitter tasks.
    async fn run(self, results_rx: mpsc::Receiver<FinalizedPing>) {
        info!("TargetWorker started for target {}", self.target_ip);

        // Move components out of self to avoid partial move errors.
        let pinger = self.pinger;
        let pinger_command_tx = self.pinger_command_tx;
        let batch_submitter = self.batch_submitter;
        let mut command_rx = self.command_rx;
        let buffer = self.buffer;

        let mut pinger_handle = tokio::spawn(pinger.run());
        let mut submitter_handle = tokio::spawn(batch_submitter.run(results_rx));

        loop {
            tokio::select! {
                Some(command) = command_rx.recv() => {
                    match command {
                        WorkerCommand::GetHealth(tx) => {
                            let len = buffer.lock().unwrap().len();
                            info!(
                                "TargetWorker for {} reporting health: buffer_size = {}",
                                self.target_ip, len
                            );
                            let health = WorkerHealth { buffer_size: len };
                            let _ = tx.send(health);
                        }
                        WorkerCommand::UpdateRole(role) => {
                            info!("TargetWorker for {} updating role to {:?}", self.target_ip, role);
                            if pinger_command_tx.send(PingerCommand::UpdateRole(role)).await.is_err() {
                                warn!("Failed to send UpdateRole command to pinger for target {}.", self.target_ip);
                            }
                        }
                        WorkerCommand::Shutdown => {
                            info!(
                                "TargetWorker for {} received shutdown command.",
                                self.target_ip
                            );
                            break; // Exit the loop to shut down
                        }
                    }
                }
                _ = &mut pinger_handle => {
                    warn!("Pinger task for {} exited unexpectedly.", self.target_ip);
                    break;
                }
                _ = &mut submitter_handle => {
                    warn!("BatchSubmitter task for {} exited unexpectedly.", self.target_ip);
                    break;
                }
            }
        }

        // When shutdown is received, abort the child tasks.
        pinger_handle.abort();
        submitter_handle.abort();
        info!("TargetWorker for {} has shut down.", self.target_ip);
    }
}
