use crate::{
    batch_submitter::BatchSubmitter, database_client::DatabaseClient, pinger::Pinger,
    ping_surge_client::PingSurgeClient,
};
use anyhow::Result;
use log::info;
use std::{net::IpAddr, sync::Arc, time::Duration};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

// A simple command enum for the TargetWorker.
#[derive(Debug)]
pub enum WorkerCommand {
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
        db_client: DatabaseClient,
    ) -> Result<TargetWorkerHandle> {
        let (command_tx, command_rx) = mpsc::channel(1);
        let (results_tx, results_rx) = mpsc::channel(100);

        let ping_client = Arc::new(PingSurgeClient::new(target_ip)?);

        let pinger = Pinger::new(
            target_ip,
            ping_rate_pps,
            Duration::from_secs(60),
            Duration::from_secs(5),
            ping_client,
            results_tx,
            db_client.clone(),
        );

        let batch_submitter = BatchSubmitter::new(
            collector_uuid,
            target_ip,
            Duration::from_secs(60),
            1_000_000,
            Duration::from_secs(24 * 3600),
            db_client,
        );

        let worker = Self {
            target_ip,
            pinger,
            batch_submitter,
            command_rx,
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
    async fn run(self, results_rx: mpsc::Receiver<crate::ping_client::PingResult>) {
        info!("TargetWorker started for target {}", self.target_ip);

        // Move components out of self to avoid partial move errors.
        let pinger = self.pinger;
        let batch_submitter = self.batch_submitter;
        let mut command_rx = self.command_rx;

        let pinger_handle = tokio::spawn(pinger.run());
        let submitter_handle = tokio::spawn(batch_submitter.run(results_rx));

        // Wait for a shutdown command.
        if let Some(WorkerCommand::Shutdown) = command_rx.recv().await {
            info!(
                "TargetWorker for {} received shutdown command.",
                self.target_ip
            );
        }

        // When shutdown is received, abort the child tasks.
        pinger_handle.abort();
        submitter_handle.abort();
        info!("TargetWorker for {} has shut down.", self.target_ip);
    }
}
