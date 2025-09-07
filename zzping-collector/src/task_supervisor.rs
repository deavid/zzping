use crate::{
    database_client::DatabaseClient,
    target_worker::{TargetWorker, TargetWorkerHandle, WorkerCommand},
};
use anyhow::Result;
use log::{error, info, warn};
use std::{
    collections::{HashMap, HashSet},
    net::IpAddr,
};
use tokio::sync::{mpsc, watch};

/// The configuration for the supervisor, received from the SessionHandler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorConfig {
    pub targets: HashSet<IpAddr>,
    pub ping_rate_pps: u64,
}

/// A command to update the TaskSupervisor's database client.
#[derive(Debug)]
pub enum ClientUpdate {
    NewClient(Box<DatabaseClient>),
    ClientLost,
}

/// The long-lived manager of the worker pool.
pub struct TaskSupervisor {
    collector_uuid: String,
    pub db_client: Option<DatabaseClient>,
    pub workers: HashMap<IpAddr, TargetWorkerHandle>,
}

impl TaskSupervisor {
    /// Creates a new `TaskSupervisor`.
    pub fn new(collector_uuid: String) -> Self {
        Self {
            collector_uuid,
            db_client: None,
            workers: HashMap::new(),
        }
    }

    /// Runs the `TaskSupervisor`'s reconciliation loop.
    pub async fn run(
        mut self,
        mut config_rx: watch::Receiver<Option<SupervisorConfig>>,
        mut client_update_rx: mpsc::Receiver<ClientUpdate>,
    ) -> Result<()> {
        info!("TaskSupervisor running.");
        loop {
            tokio::select! {
                Ok(_) = config_rx.changed() => {
                    let config = (*config_rx.borrow()).clone();
                    self.reconcile(config).await;
                }
                Some(update) = client_update_rx.recv() => {
                    match update {
                        ClientUpdate::NewClient(client) => {
                            info!("TaskSupervisor received new database client.");
                            self.db_client = Some(*client);
                            let config = (*config_rx.borrow()).clone();
                            self.reconcile(config).await;
                        }
                        ClientUpdate::ClientLost => {
                            info!("TaskSupervisor notified of lost database client.");
                            self.db_client = None;
                        }
                    }
                }
                else => {
                    info!("All channels closed. TaskSupervisor shutting down.");
                    break;
                }
            }
        }

        info!("Shutting down all workers.");
        self.reconcile(None).await;

        Ok(())
    }

    /// Compares the desired state with the actual state and takes action.
    pub async fn reconcile(&mut self, config: Option<SupervisorConfig>) {
        let (desired_targets, ping_rate_pps) = match config {
            Some(c) => (c.targets, c.ping_rate_pps),
            None => (HashSet::new(), 0),
        };
        let current_workers = self.workers.keys().cloned().collect::<HashSet<_>>();

        for &target_ip in desired_targets.difference(&current_workers) {
            if let Some(db_client) = &self.db_client {
                info!("TaskSupervisor: Adding worker for target {target_ip}");
                match TargetWorker::new(
                    self.collector_uuid.clone(),
                    target_ip,
                    ping_rate_pps,
                    db_client.clone(),
                ) {
                    Ok(handle) => {
                        self.workers.insert(target_ip, handle);
                    }
                    Err(e) => {
                        error!("Failed to create TargetWorker for {target_ip}: {e}. This may be a permissions issue.");
                    }
                }
            } else {
                info!("TaskSupervisor: Deferring worker creation for {target_ip}, no database client.");
            }
        }

        for &target_ip in current_workers.difference(&desired_targets) {
            info!("TaskSupervisor: Removing worker for target {target_ip}");
            if let Some(worker_handle) = self.workers.remove(&target_ip) {
                if worker_handle
                    .command_tx
                    .send(WorkerCommand::Shutdown)
                    .await
                    .is_err()
                {
                    warn!("Failed to send shutdown command to worker for target {target_ip}");
                }
                worker_handle.task_handle.abort();
            }
        }
    }
}
