use crate::{
    collector_service::CachedIntent,
    database_client::DatabaseClientTrait,
    target_worker::{TargetWorker, TargetWorkerHandle, WorkerCommand},
};
use anyhow::Result;
use futures::future::join_all;
use log::{error, info, warn};
use std::{
    collections::{HashMap, HashSet},
    net::IpAddr,
    sync::Arc,
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
    pub swap_at_nanos: Option<u64>,
    pub use_mock_ping_client: bool,
}

/// A command to update the TaskSupervisor's database client.
// Removed #[derive(Debug)]
pub enum ClientUpdate {
    NewClient(Arc<dyn DatabaseClientTrait>),
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
    pub db_client: Option<Arc<dyn DatabaseClientTrait>>,
    pub workers: HashMap<IpAddr, TargetWorkerHandle>,
    current_role: CollectorRole,
    current_config: Option<SupervisorConfig>,
    // Health heartbeat interval in milliseconds. Defaults to 1000ms.
    health_interval_ms: u64,
    // Cancellation handle for a scheduled swap (if any)
    swap_cancel_tx: Option<oneshot::Sender<()>>,
    // Test-only: Channel to report worker count after reconciliation
    worker_count_tx: Option<mpsc::Sender<usize>>,
}

impl TaskSupervisor {
    /// Creates a new `TaskSupervisor`.
    pub fn new(
        collector_uuid: String,
        health_interval_ms: u64,
        cached_intent: Option<CachedIntent>,
    ) -> Self {
        Self::new_with_worker_count_tx(collector_uuid, health_interval_ms, cached_intent, None)
    }

    /// Test-only: Creates a new `TaskSupervisor` with a channel to report worker count.
    pub fn new_with_worker_count_tx(
        collector_uuid: String,
        health_interval_ms: u64,
        cached_intent: Option<CachedIntent>,
        worker_count_tx: Option<mpsc::Sender<usize>>,
    ) -> Self {
        let initial_config = cached_intent.map(|intent| SupervisorConfig {
            targets: intent.targets,
            ping_rate_pps: intent.ping_rate_pps,
            role: CollectorRole::Primary, // Default to Primary if loaded from cache
            swap_at_nanos: None,
            use_mock_ping_client: false, // This will be overridden by actual config
        });

        Self {
            collector_uuid,
            db_client: None,
            workers: HashMap::new(),
            current_role: CollectorRole::Standby,
            current_config: initial_config,
            health_interval_ms,
            swap_cancel_tx: None,
            worker_count_tx,
        }
    }

    /// Runs the `TaskSupervisor`'s reconciliation loop.
    pub async fn run(
        mut self,
        mut config_rx: watch::Receiver<Option<SupervisorConfig>>,
        mut client_update_rx: mpsc::Receiver<ClientUpdate>,
        health_report_tx: mpsc::Sender<HealthReport>,
        mut supervisor_shutdown_rx: mpsc::Receiver<SupervisorShutdown>,
        mut fsync_rx: mpsc::Receiver<u64>,
    ) -> Result<()> {
        info!("TaskSupervisor running.");
        // Allow tests to override the health heartbeat interval via environment
        // variable so integration tests can run quickly without changing
        // production behaviour. Value is in milliseconds.
        // Use the configured health interval (in milliseconds). Tests should
        // pass a small value via TaskSupervisor::new when bootstrapping.
        let mut health_interval =
            tokio::time::interval(Duration::from_millis(self.health_interval_ms));
        // Channel used by scheduled swap sleepers to notify the supervisor
        let (swap_notify_tx, mut swap_notify_rx) = mpsc::channel::<CollectorRole>(1);

        // Initial reconcile if we have cached config
        if self.current_config.is_some() {
            self.reconcile(self.current_config.clone()).await;
        }

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
                    let new_config = (*config_rx.borrow()).clone();
                    // Cancel any previously scheduled swap when a new config arrives.
                    if let Some(cancel) = self.swap_cancel_tx.take() {
                        let _ = cancel.send(());
                    }
                    // Update current_config and then reconcile
                    self.current_config = new_config.clone();
                    self.reconcile(new_config.clone()).await;
                    // If the config includes a swap_at_nanos, schedule a swap notifier
                    if let Some(swap_at) = new_config.as_ref().and_then(|c| c.swap_at_nanos) {
                        // Compute duration until swap; if in the past, schedule immediate
                        let now_ns = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_nanos() as u64;
                        let dur_nanos = swap_at.saturating_sub(now_ns);
                        let dur = Duration::from_nanos(dur_nanos);
                        let swap_tx = swap_notify_tx.clone();
                        let role_to_apply_at_swap = new_config.clone().unwrap().role; // Capture the role to apply
                        let (cancel_tx, cancel_rx) = oneshot::channel::<()>();
                        self.swap_cancel_tx = Some(cancel_tx);
                        tokio::spawn(async move {
                            tokio::select! {
                                _ = tokio::time::sleep(dur) => {
                                    let _ = swap_tx.send(role_to_apply_at_swap).await;
                                }
                                _ = cancel_rx => {
                                    // cancelled
                                }
                            }
                        });
                    }
                }
                Some(update) = client_update_rx.recv() => {
                    match update {
                        ClientUpdate::NewClient(client) => {
                            info!("TaskSupervisor received new database client.");
                            self.db_client = Some(client);
                            // Trigger reconciliation with the current config (from cache or last received)
                            self.reconcile(self.current_config.clone()).await;
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
                Some(fsync_nanos) = fsync_rx.recv() => {
                    info!("TaskSupervisor received fsync notification: {}", fsync_nanos);
                    // Broadcast prune to all workers. Each worker will forward to its BatchSubmitter.
                    for worker_handle in self.workers.values() {
                        if worker_handle
                            .command_tx
                            .send(WorkerCommand::PruneByFsync(fsync_nanos)).await
                            .is_err()
                        {
                            warn!("Failed to send PruneByFsync to worker");
                        }
                    }
                }
                // Received a scheduled swap notification: apply role change now.
                Some(swap_role) = swap_notify_rx.recv() => {
                    info!("Scheduled swap triggered: applying role {:?}", swap_role);
                    self.current_role = swap_role;
                    // Update self.current_config with the new role before reconciling
                    if let Some(ref mut config) = self.current_config {
                        config.role = swap_role;
                    }
                    self.reconcile(self.current_config.clone()).await;
                }
                else => {
                    info!("All channels closed. TaskSupervisor shutting down.");
                    break;
                }
            }
        }

        info!("TaskSupervisor exited main loop.");

        Ok(())
    }

    /// Shuts down all workers gracefully.
    async fn shutdown(&mut self) {
        info!("SHUTDOWN_LOG: TaskSupervisor::shutdown started.");
        let handles: Vec<_> = self.workers.drain().map(|(_, handle)| handle).collect();
        info!("SHUTDOWN_LOG: Drained {} workers.", handles.len());
        let shutdown_commands = handles
            .iter()
            .map(|handle| {
                info!("SHUTDOWN_LOG: Sending Shutdown to worker.");
                handle.command_tx.send(WorkerCommand::Shutdown)
            })
            .collect::<Vec<_>>();

        join_all(shutdown_commands).await;
        info!("SHUTDOWN_LOG: All worker shutdown commands sent.");

        let shutdown_futures = handles.into_iter().map(|handle| handle.task_handle);
        join_all(shutdown_futures).await;
        info!("SHUTDOWN_LOG: All workers have been joined.");
        info!("All workers have been shut down.");
    }

    /// Compares the desired state with the actual state and takes action.
    pub async fn reconcile(&mut self, config: Option<SupervisorConfig>) {
        let (desired_targets, ping_rate_pps, new_role, _swap_at_nanos, use_mock_ping_client) =
            match config {
                Some(c) => (
                    c.targets,
                    c.ping_rate_pps,
                    c.role,
                    c.swap_at_nanos,
                    c.use_mock_ping_client,
                ),
                None => (HashSet::new(), 0, CollectorRole::Standby, None, false),
            };
        self.current_role = new_role;

        let current_workers = self.workers.keys().cloned().collect::<HashSet<_>>();

        self.reconcile_workers(&current_workers, &desired_targets)
            .await;

        // Add new workers
        for &target_ip in desired_targets.difference(&current_workers) {
            if let Some(db_client) = self.db_client.clone() {
                info!("TaskSupervisor: Adding worker for target {target_ip}");
                let worker_result = if use_mock_ping_client {
                    // For tests, use MockPingClient
                    use crate::ping_client::MockPingClient;
                    let ping_client = std::sync::Arc::new(MockPingClient::new(target_ip));
                    TargetWorker::new_with_ping_client(
                        self.collector_uuid.clone(),
                        target_ip,
                        ping_rate_pps,
                        db_client.clone(),
                        ping_client,
                    )
                } else {
                    // For production, use real PingSurgeClient
                    TargetWorker::new(
                        self.collector_uuid.clone(),
                        target_ip,
                        ping_rate_pps,
                        db_client.clone(),
                    )
                };
                match worker_result {
                    Ok(handles) => {
                        self.workers.insert(target_ip, handles.handle);
                    }
                    Err(e) => {
                        error!(
                            "Failed to create TargetWorker for {target_ip}: {e}. This may be a permissions issue."
                        );
                    }
                }
            } else {
                info!(
                    "TaskSupervisor: Deferring worker creation for {target_ip}, no database client."
                );
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

        // Report worker count for testing
        if let Some(tx) = &self.worker_count_tx {
            let _ = tx.send(self.workers.len()).await;
        }
    }

    /// Removes workers that are in `current` but not in `desired`.
    async fn reconcile_workers(&mut self, current: &HashSet<IpAddr>, desired: &HashSet<IpAddr>) {
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
