use crate::{
    database_client::DatabaseClient,
    pinger::FinalizedPing,
    target_worker::{TargetWorker, TargetWorkerHandle, WorkerCommand},
};
use anyhow::Result;
use futures::future::join_all;
use log::{error, info, warn};
use std::{
    collections::{HashMap, HashSet},
    net::IpAddr,
    time::Duration,
};
use tokio::sync::{mpsc, oneshot, watch};
use zzping_proto::zzping::CollectorRole;

/// The configuration for the supervisor, received from the SessionHandler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorConfig {
    pub targets: HashSet<IpAddr>,
    pub ping_rate_pps: u64,
    pub role: CollectorRole,
}

/// A command to update the TaskSupervisor's database client.
#[derive(Debug)]
pub enum ClientUpdate {
    NewClient(Box<DatabaseClient>),
    ClientLost,
}

/// A command to shut down the supervisor gracefully.
#[derive(Debug)]
pub struct SupervisorShutdown {
    pub ack_sender: oneshot::Sender<()>,
}

/// A report of the system's health, sent from the supervisor to the service.
#[derive(Debug, Clone)]
pub struct HealthReport {
    pub total_buffer_size: usize,
    pub role: CollectorRole,
    pub fatal_errors: Vec<String>,
}

/// The long-lived manager of the worker pool.
pub struct TaskSupervisor {
    collector_uuid: String,
    pub db_client: Option<DatabaseClient>,
    pub workers: HashMap<IpAddr, TargetWorkerHandle>,
    current_role: CollectorRole,
    // A channel to send the data injection sender to the test harness.
    test_data_tx_sender: Option<mpsc::Sender<mpsc::Sender<FinalizedPing>>>,
}

impl TaskSupervisor {
    /// Creates a new `TaskSupervisor`.
    pub fn new(
        collector_uuid: String,
        test_data_tx_sender: Option<mpsc::Sender<mpsc::Sender<FinalizedPing>>>,
    ) -> Self {
        Self {
            collector_uuid,
            db_client: None,
            workers: HashMap::new(),
            current_role: CollectorRole::Standby,
            test_data_tx_sender,
        }
    }

    /// Runs the `TaskSupervisor`'s reconciliation loop.
    pub async fn run(
        mut self,
        mut config_rx: watch::Receiver<Option<SupervisorConfig>>,
        mut client_update_rx: mpsc::Receiver<ClientUpdate>,
        health_report_tx: mpsc::Sender<HealthReport>,
        mut supervisor_shutdown_rx: mpsc::Receiver<SupervisorShutdown>,
    ) -> Result<()> {
        info!("TaskSupervisor running.");
        let mut health_interval = tokio::time::interval(Duration::from_secs(1));

        loop {
            tokio::select! {
                _ = health_interval.tick() => {
                    let mut total_buffer_size = 0;
                    for worker_handle in self.workers.values() {
                        let (tx, rx) = oneshot::channel();
                        if worker_handle.command_tx.send(WorkerCommand::GetHealth(tx)).await.is_ok()
                            && let Ok(health) = rx.await {
                                total_buffer_size += health.buffer_size;
                            }
                    }

                    // NOTE: Fatal error reporting is not yet implemented.
                    let report = HealthReport {
                        total_buffer_size,
                        role: self.current_role,
                        fatal_errors: vec![],
                    };

                    if health_report_tx.send(report).await.is_err() {
                        warn!("Health report channel closed. Cannot send health reports.");
                    }
                }
                Ok(_) = config_rx.changed() => {
                    info!("TaskSupervisor received new config: {:?}", config_rx.borrow());
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
                Some(command) = supervisor_shutdown_rx.recv() => {
                    info!("TaskSupervisor received shutdown command.");
                    self.shutdown().await;
                    // Acknowledge that the shutdown is complete.
                    command.ack_sender.send(()).ok();
                    break;
                }
                else => {
                    info!("All channels closed. TaskSupervisor shutting down.");
                    break;
                }
            }
        }

        info!("Shutting down all workers.");
        self.shutdown().await;

        Ok(())
    }

    /// Shuts down all workers gracefully.
    async fn shutdown(&mut self) {
        let current_workers = self.workers.keys().cloned().collect::<HashSet<_>>();
        self.reconcile_workers(&current_workers, &HashSet::new())
            .await;
    }

    /// Compares the desired state with the actual state and takes action.
    pub async fn reconcile(&mut self, config: Option<SupervisorConfig>) {
        let (desired_targets, ping_rate_pps, new_role) = match config {
            Some(c) => (c.targets, c.ping_rate_pps, c.role),
            None => (HashSet::new(), 0, CollectorRole::Standby),
        };
        self.current_role = new_role;

        let current_workers = self.workers.keys().cloned().collect::<HashSet<_>>();

        self.reconcile_workers(&current_workers, &desired_targets)
            .await;

        // Add new workers
        for &target_ip in desired_targets.difference(&current_workers) {
            if let Some(db_client) = &self.db_client {
                info!("TaskSupervisor: Adding worker for target {target_ip}");
                match TargetWorker::new(
                    self.collector_uuid.clone(),
                    target_ip,
                    ping_rate_pps,
                    db_client.clone(),
                ) {
                    Ok(handles) => {
                        if let Some(sender) = &self.test_data_tx_sender {
                            if sender.send(handles.data_tx).await.is_err() {
                                warn!("Failed to send worker data_tx to test harness. Test might hang.");
                            }
                        }
                        self.workers.insert(target_ip, handles.handle);
                    }
                    Err(e) => {
                        error!("Failed to create TargetWorker for {target_ip}: {e}. This may be a permissions issue.");
                    }
                }
            } else {
                info!("TaskSupervisor: Deferring worker creation for {target_ip}, no database client.");
            }
        }

        // Update role for all current workers
        info!(
            "TaskSupervisor: Updating role for all workers to {:?}",
            self.current_role
        );
        for worker_handle in self.workers.values() {
            if worker_handle
                .command_tx
                .send(WorkerCommand::UpdateRole(self.current_role))
                .await
                .is_err()
            {
                warn!("Failed to send UpdateRole command to a worker.");
            }
        }
    }

    /// Removes workers that are in `current` but not in `desired`.
    async fn reconcile_workers(
        &mut self,
        current: &HashSet<IpAddr>,
        desired: &HashSet<IpAddr>,
    ) {
        let workers_to_remove = current.difference(desired);
        let mut shutdown_handles = vec![];

        for &target_ip in workers_to_remove {
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
                // Collect the handle to await it later.
                shutdown_handles.push(async move {
                    if let Err(e) = worker_handle.task_handle.await {
                        warn!(
                            "Worker task for target {} panicked during shutdown: {:?}",
                            target_ip, e
                        );
                    }
                });
            }
        }

        // Await all the workers that were shut down.
        join_all(shutdown_handles).await;
    }
}
