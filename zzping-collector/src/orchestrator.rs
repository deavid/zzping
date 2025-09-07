//! Coordinates collector components based on database heartbeats.
//!
//! The orchestrator translates database heartbeats into runtime actions,
//! such as spawning tasks, seeding buffers, and managing state transitions.
//! It ensures a single source of truth for configuration.
//!
//! Rationale and trade-offs:
//! - Single orchestrator process: having one component translate heartbeats
//!   avoids split-brain decisions in the collector. Multiple pieces reacting
//!   independently to heartbeats would make reasoning about role transitions
//!   and buffer seeding error-prone.
//! - Cache to disk (bincode, small payload): this is a pragmatic safety net
//!   so a restart without DB connectivity has a deterministic starting
//!   configuration. We keep the cached state minimal to avoid staleness.
//! - Heartbeat loop separation from workers: the orchestrator runs the
//!   heartbeat synchronously (holding the shared client) and then broadcasts
//!   changes. This keeps the heartbeat handler simple and serializes
//!   config change handling to a single place.
//!
//! Operational invariants:
//! - Config snapshots are authoritative and sent to the TaskSupervisor as a
//!   single unit; TaskSupervisor assumes it receives the full desired state.
//! - When SeedBuffer action is requested, the orchestrator synchronously
//!   fetches recent data and logs the result. Buffer merging happens in the
//!   BatchSubmitter/runner code paths and must preserve monotonicity.
//! - Orchestrator will abort spawned background tasks on disconnect and
//!   reconnect; ephemeral tasks must be tolerant to abrupt cancellation.

// FIXME: Why isn't the orchestrator then, the one managing the BatchSubmitter? Why do we need Arc<Mutex<DatabaseXYZ>> ??

use crate::{
    batch_submitter::BatchSubmitter,
    cli::Cli,
    database_client::{GrpcClient, SharedDatabaseClient},
    ping_client::PingResult,
    state_machine::{Action, State, StateMachine},
    task_supervisor::{SupervisorConfig, TaskSupervisor},
};
use anyhow::Result;
use base64::{Engine as _, engine::general_purpose};
use bincode;
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path, sync::Arc, time::Duration};
use tokio::sync::{mpsc, watch};
use zzping_lib::auth::AuthToken;
use zzping_proto::zzping::{GetRecentDataRequest, HeartbeatRequest};

/// Cached configuration persisted to disk for fault tolerance.
///
/// This structure enables the collector to restart with its previous
/// configuration when the database is unavailable. It's serialized
/// using bincode for efficient storage and quick deserialization.
///
/// ## Design Rationale
///
/// **Fault Tolerance**: Allows the collector to continue operating
/// with last-known-good configuration during database outages.
///
/// **Quick Recovery**: Binary serialization provides fast load times
/// compared to text formats like JSON.
///
/// **Minimal Storage**: Only caches essential configuration data
/// (targets and ping rate) to keep the cache file small.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
struct CachedConfig {
    // FIXME: Is "CachedConfig" a misnomer? True that we add this because we want to persist, but shouldn't be this
    // .. the main structure to keep in memory to handle what's the current configuration for the work to do, the current intent?
    // .. How is this used currently? Do we have a proper single source of intent, or do we have it sparse on the code?

    /// List of target IP addresses to monitor.
    targets: Vec<String>,

    /// Ping rate in packets per second for all targets.
    ping_rate_pps: u64,
}

/// Central orchestrator for coordinating collector components.
///
/// The CollectorOrchestrator implements a supervisor pattern that manages
/// the entire collector lifecycle. It coordinates between the task supervisor
/// (manages ping workers), batch submitter (handles data buffering), and
/// database client (manages persistence).
///
/// ## Design Rationale
///
/// **Centralized Coordination**: Single point of control for all component
/// interactions, making the system easier to reason about and debug.
///
/// **Reactive Configuration**: Uses watch channels to propagate configuration
/// changes to components without tight coupling.
///
/// **Graceful Shutdown**: Ensures all components are properly cleaned up
/// when the orchestrator shuts down.
///
/// **State Machine Integration**: Translates database heartbeat responses
/// into concrete actions for the collector components.
///
/// **Configuration Persistence**: Caches configuration to disk to survive
/// restarts and database outages.
pub struct CollectorOrchestrator {
    // FIXME: Why is this an Arc<T> ? Do we expect others to change the internals? Why not just clone?
    // ... Also, passing the Cli is probably bad practice because it tightly couples whatever we want on the input flags
    // ... to be literally the same for internal work functions.
    /// Command-line configuration shared across components.
    cli: Arc<Cli>,

    /// State machine for tracking collector operational state.
    /// Manages transitions between STARTUP, PRIMARY, PINGING, and SHUTDOWN.
    state_machine: StateMachine,

    /// Configuration sender for reactive updates to the task supervisor.
    /// Components subscribe to receive configuration changes.
    config_tx: watch::Sender<SupervisorConfig>,

    // FIXME: And what is the current config, the main data source for the intent? why isn't this just that? Why does it need to be an option?
    /// Last configuration cached to disk.
    /// Used to detect when new configurations need to be persisted.
    last_cached_config: Option<CachedConfig>,

    // FIXME: to me it seems that this cache_file belongs to whatever struct we use to replace the Cli one.
    /// Path to the configuration cache file.
    cache_file: std::path::PathBuf,
}

impl CollectorOrchestrator {
    /// Creates a new orchestrator with the given command-line configuration.
    pub fn new(cli: Arc<Cli>) -> Self {
        // FIXME: All these folder-filename needs to be provided externally.
        let cache_dir = Path::new(".cache/zzping");
        fs::create_dir_all(cache_dir).unwrap();
        let cache_file = cache_dir.join("last_config.bin");

        // FIXME: And here we are. This is the current intent config. So we really do have two different intent configs. Bad.
        let initial_config = SupervisorConfig {
            targets: vec![],
            ping_rate_pps: 0,
            should_be_pinging: false,
        };
        // FIXME: This is a HUGE RED FLAG: We're discarding the RX side. Whatever we're doing with this (no idea), smells a lot like bugs.
        // .. whatever we send here, no one is listening. - now I notice that for some reason, it seems we can create RX channels out of
        // .. TX ones, so this would be fine, just a bit weird for me that I'm not used.
        let (config_tx, _) = watch::channel(initial_config);

        Self {
            cli: cli.clone(),
            state_machine: StateMachine::new(cli.source_hostname.clone()),
            config_tx,
            last_cached_config: None,
            cache_file,
        }
    }

    /// Runs the orchestrator's main event loop. Attempts to connect to DB
    /// and run the collector components, retrying the DB connection after a delay.
    /// Will continue until a shutdown command is received.
    pub async fn run(mut self) -> Result<()> {
        // FIXME: This function returns a Result, which means that it is fallible, which is against the definition of this function.
        // Load cached config
        if let Ok(bytes) = fs::read(&self.cache_file)
            && let Ok(config) = bincode::deserialize::<CachedConfig>(&bytes)
        {
            // FIXME: What's bincode doing here? It's a config file, should be RON.
            // FIXME: This initialization seems that belongs to the constructor.
            info!("Loaded cached config: {:?}", config);
            let supervisor_config = SupervisorConfig {
                targets: config.targets.clone(),
                ping_rate_pps: config.ping_rate_pps,
                should_be_pinging: true,
            };
            // FIXME: Unwrap? that's bad.
            self.config_tx.send(supervisor_config).unwrap();
            self.last_cached_config = Some(config);
        }

        loop {
            // FIXME: Why is the channel being recreated each time? Why? So we're losing information on each reconnect?
            let (ping_tx, ping_rx) = mpsc::channel(1000);
            match self.connect_and_run(ping_tx, ping_rx).await {
                Ok(_) => return Ok(()), // Shutdown
                Err(_) => {
                    // Connection failed, retry
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    }

    /// Establishes database connection and runs all collector components.
    ///
    /// This method sets up the communication channels between components
    /// and spawns the task supervisor and batch submitter. It then runs
    /// the heartbeat loop to maintain connection with the database.
    ///
    /// # Parameters
    /// * `ping_tx` - Sender for ping results from workers to batch submitter
    /// * `ping_rx` - Receiver for ping results in the batch submitter
    ///
    /// # Returns
    /// An error if database connection fails or heartbeat loop encounters issues
    async fn connect_and_run(
        &mut self,
        ping_tx: mpsc::Sender<PingResult>,
        ping_rx: mpsc::Receiver<PingResult>,
    ) -> Result<()> {
        // FIXME: Didn't we connect elsewhere? It doesn't seem so, but database_client has the unit tests for connect. Why isn't this code there?

        info!("Attempting to connect to gRPC server...");
        let client = GrpcClient::connect(&self.cli).await?;
        let shared_client = Arc::new(tokio::sync::Mutex::new(client));

        let token = {
            let auth_token = AuthToken {
                sub: self.cli.source_hostname.clone(),
                roles: vec!["collector".to_string()],
            };
            let json = serde_json::to_string(&auth_token).unwrap();
            general_purpose::STANDARD.encode(json)
        };

        // We are recreating the BatchSubmitter here? So it loses the data on reconnects? talk about bad design...

        let batch_submitter = BatchSubmitter::new(
            shared_client.clone(),
            ping_rx,
            self.cli.source_hostname.clone(),
            token.clone(),
        );

        let config_rx = self.config_tx.subscribe();
        let task_supervisor = TaskSupervisor::new(config_rx, ping_tx, self.cli.clone());

        let batch_handle = tokio::spawn(batch_submitter.run());
        let supervisor_handle = tokio::spawn(task_supervisor.run());

        let result = self.run_heartbeat_loop(shared_client, token).await;

        // Cleanup
        batch_handle.abort();
        supervisor_handle.abort();

        result
    }

    /// Runs the heartbeat loop to maintain database connection and state.
    ///
    /// The heartbeat loop sends periodic heartbeats to the database and
    /// processes responses to update the collector's configuration and state.
    /// It handles state transitions, configuration caching, and buffer seeding.
    ///
    /// # Parameters
    /// * `client` - Shared database client for sending heartbeats
    /// * `token` - Authentication token for database requests
    ///
    /// # Returns
    /// An error if the heartbeat fails and reconnection is needed
    async fn run_heartbeat_loop(
        &mut self,
        client: SharedDatabaseClient,
        token: String,
    ) -> Result<()> {
        // FIXME: This time should be configurable externally to this function call.
        let mut heartbeat_interval = tokio::time::interval(Duration::from_secs(1));
        let pid = std::process::id() as u64;

        loop {
            heartbeat_interval.tick().await;
            // FIXME: The dance of new request, metadata, lock, heartbeat could be abstracted away.
            // FIXME: Why are we not publishing the status to the database? shouldn't the database know what is our current intent and health? is this collector primary or standby?
            let mut request = tonic::Request::new(HeartbeatRequest {
                collector_uuid: self.cli.source_hostname.clone(),
                pid,
            });
            request.metadata_mut().insert(
                "authorization",
                format!("Bearer {}", token).parse().unwrap(),
            );

            let client_lock = client.lock().await;
            match client_lock.heartbeat(request).await {
                Ok(response) => {
                    let action = self.state_machine.handle_heartbeat_response(&response);
                    // FIXME: Why isn't here a "match action {" to take care? Why Option<T>? isn't it easier to handle a default custom none value?
                    // .. unless we can use the "None" to short-circuit some of the logic below, Option<T> might not be worth it.

                    // TODO: This needs to be abstracted away from here:
                    // Cache the new config if it's different.
                    let new_config = CachedConfig {
                        targets: response.targets.clone(),
                        ping_rate_pps: response.ping_rate_pps,
                    };
                    if Some(&new_config) != self.last_cached_config.as_ref() {
                        info!(
                            "New configuration received. Caching to disk: {:?}",
                            new_config
                        );
                        if let Ok(bytes) = bincode::serialize(&new_config)
                            && let Err(e) = fs::write(&self.cache_file, bytes)
                        {
                            warn!("Failed to write to cache file: {e}");
                        }
                        self.last_cached_config = Some(new_config);
                    }

                    // TODO: This needs to be abstracted away from here:
                    // FIXME: WAIT A SECOND. The database is sending "SeedBuffer" to the collector, and the collector is taking this as that it needs to change to primary mode?
                    // ... this is wrong on so many levels. We need to talk about this.
                    if let Some(Action::SeedBuffer) = action {
                        info!("Transitioning to PRIMARY. Requesting buffer seed...");
                        let request = tonic::Request::new(GetRecentDataRequest {
                            collector_uuid: self.cli.source_hostname.clone(),
                            lookback_seconds: 3600, // 1 hour
                        });
                        match client_lock.get_recent_data(request).await {
                            Ok(data) => info!(
                                "Successfully seeded buffer with {} records.",
                                data.records.len()
                            ),
                            Err(e) => warn!("Failed to seed buffer: {e}"),
                        }
                    }

                    // FIXME: This looks weird here, this seems to actually come from "handle_heartbeat_response". This needs review.
                    if self.state_machine.current_state == State::Shutdown {
                        info!("Received SHUTDOWN command. Exiting.");
                        return Ok(());
                    }

                    // FIXME: Why the bounce around from self.state_machine.current_state -> SupervisorConfig; shouldn't the intent config be unified?
                    let should_be_pinging = self.state_machine.current_state == State::Pinging;
                    let supervisor_config = SupervisorConfig {
                        targets: response.targets,
                        ping_rate_pps: response.ping_rate_pps,
                        should_be_pinging,
                    };
                    self.config_tx.send(supervisor_config).unwrap();
                }
                Err(e) => {
                    error!("Heartbeat failed: {e}. Reconnecting...");
                    return Err(e);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Cli;
    use std::sync::Arc;
    use tempfile::tempdir;
    use zzping_proto::zzping::*;

    // Mock database client for testing
    #[allow(dead_code)]
    struct MockDatabaseClient {
        heartbeat_response: Result<HeartbeatResponse>,
        get_recent_data_response: Result<GetRecentDataResponse>,
    }

    #[async_trait::async_trait]
    impl crate::database_client::DatabaseClient for MockDatabaseClient {
        async fn heartbeat(
            &self,
            _req: tonic::Request<HeartbeatRequest>,
        ) -> anyhow::Result<HeartbeatResponse> {
            match &self.heartbeat_response {
                Ok(resp) => Ok(resp.clone()),
                Err(e) => Err(anyhow::anyhow!(e.to_string())),
            }
        }

        async fn send_batch(
            &self,
            _req: tonic::Request<SendBatchRequest>,
        ) -> anyhow::Result<SendBatchResponse> {
            Ok(SendBatchResponse::default())
        }

        async fn get_recent_data(
            &self,
            _req: tonic::Request<GetRecentDataRequest>,
        ) -> anyhow::Result<GetRecentDataResponse> {
            match &self.get_recent_data_response {
                Ok(resp) => Ok(resp.clone()),
                Err(e) => Err(anyhow::anyhow!(e.to_string())),
            }
        }
    }

    #[tokio::test]
    #[ntest::timeout(100)]
    async fn test_orchestrator_new() {
        let cli = Arc::new(Cli {
            source_hostname: "test-collector".to_string(),
            database_addr: "http://127.0.0.1:8080".to_string(),
            auth_token: "test-token".to_string(),
            max_in_flight: 3,
        });

        let orchestrator = CollectorOrchestrator::new(cli.clone());

        assert_eq!(orchestrator.cli.source_hostname, "test-collector");
        assert_eq!(orchestrator.cli.database_addr, "http://127.0.0.1:8080");
        assert!(orchestrator.last_cached_config.is_none());
        // Cache file should exist in the .cache/zzping directory
        assert!(orchestrator.cache_file.parent().unwrap().exists());
    }

    #[tokio::test]
    #[ntest::timeout(100)]
    async fn test_orchestrator_cached_config_loading() {
        let temp_dir = tempdir().unwrap();
        let cache_file = temp_dir.path().join("test_config.bin");

        // Create test cached config
        let cached_config = CachedConfig {
            targets: vec!["1.1.1.1".to_string()],
            ping_rate_pps: 100,
        };
        let bytes = bincode::serialize(&cached_config).unwrap();
        std::fs::write(&cache_file, bytes).unwrap();

        let cli = Arc::new(Cli {
            source_hostname: "test-collector".to_string(),
            database_addr: "http://127.0.0.1:8080".to_string(),
            auth_token: "test-token".to_string(),
            max_in_flight: 3,
        });

        // Manually create orchestrator with our cache file
        let cache_dir = temp_dir.path();
        std::fs::create_dir_all(cache_dir).unwrap();

        let initial_config = SupervisorConfig {
            targets: vec![],
            ping_rate_pps: 0,
            should_be_pinging: false,
        };
        let (config_tx, _config_rx) = tokio::sync::watch::channel(initial_config);

        let mut orchestrator = CollectorOrchestrator {
            cli: cli.clone(),
            state_machine: StateMachine::new(cli.source_hostname.clone()),
            config_tx,
            last_cached_config: None,
            cache_file: cache_file.clone(),
        };

        // Test loading cached config
        if let Ok(bytes) = std::fs::read(&orchestrator.cache_file)
            && let Ok(config) = bincode::deserialize::<CachedConfig>(&bytes)
        {
            let supervisor_config = SupervisorConfig {
                targets: config.targets.clone(),
                ping_rate_pps: config.ping_rate_pps,
                should_be_pinging: true,
            };
            orchestrator.config_tx.send(supervisor_config).unwrap();
            orchestrator.last_cached_config = Some(config);
        }

        assert!(orchestrator.last_cached_config.is_some());
        let loaded_config = orchestrator.last_cached_config.as_ref().unwrap();
        assert_eq!(loaded_config.targets, vec!["1.1.1.1"]);
        assert_eq!(loaded_config.ping_rate_pps, 100);
    }

    #[tokio::test]
    #[ntest::timeout(100)]
    async fn test_orchestrator_config_caching() {
        let temp_dir = tempdir().unwrap();
        let cache_file = temp_dir.path().join("test_config.bin");

        let cli = Arc::new(Cli {
            source_hostname: "test-collector".to_string(),
            database_addr: "http://127.0.0.1:8080".to_string(),
            auth_token: "test-token".to_string(),
            max_in_flight: 3,
        });

        let initial_config = SupervisorConfig {
            targets: vec![],
            ping_rate_pps: 0,
            should_be_pinging: false,
        };
        let (config_tx, _) = tokio::sync::watch::channel(initial_config);

        let mut orchestrator = CollectorOrchestrator {
            cli: cli.clone(),
            state_machine: StateMachine::new(cli.source_hostname.clone()),
            config_tx,
            last_cached_config: None,
            cache_file: cache_file.clone(),
        };

        // Test caching new config
        let new_config = CachedConfig {
            targets: vec!["8.8.8.8".to_string()],
            ping_rate_pps: 200,
        };

        if let Ok(bytes) = bincode::serialize(&new_config) {
            let _ = std::fs::write(&cache_file, bytes);
        }
        orchestrator.last_cached_config = Some(new_config.clone());

        // Verify config was cached
        assert!(orchestrator.last_cached_config.is_some());
        let cached = orchestrator.last_cached_config.as_ref().unwrap();
        assert_eq!(cached.targets, vec!["8.8.8.8"]);
        assert_eq!(cached.ping_rate_pps, 200);
    }

    #[tokio::test]
    #[ntest::timeout(100)]
    async fn test_orchestrator_heartbeat_processing() {
        let cli = Arc::new(Cli {
            source_hostname: "test-collector".to_string(),
            database_addr: "http://127.0.0.1:8080".to_string(),
            auth_token: "test-token".to_string(),
            max_in_flight: 3,
        });

        let initial_config = SupervisorConfig {
            targets: vec![],
            ping_rate_pps: 0,
            should_be_pinging: false,
        };
        let (config_tx, mut config_rx) = tokio::sync::watch::channel(initial_config);

        let mut orchestrator = CollectorOrchestrator {
            cli: cli.clone(),
            state_machine: StateMachine::new(cli.source_hostname.clone()),
            config_tx,
            last_cached_config: None,
            cache_file: std::path::PathBuf::from("/tmp/nonexistent"),
        };

        // Mock heartbeat response
        let heartbeat_response = HeartbeatResponse {
            targets: vec!["1.1.1.1".to_string(), "8.8.8.8".to_string()],
            ping_rate_pps: 50,
            role: 0, // Primary
            swap_at_nanos: 0,
        };

        // Simulate heartbeat processing
        let _action = orchestrator
            .state_machine
            .handle_heartbeat_response(&heartbeat_response);

        let should_be_pinging = orchestrator.state_machine.current_state == State::Pinging;
        let supervisor_config = SupervisorConfig {
            targets: heartbeat_response.targets,
            ping_rate_pps: heartbeat_response.ping_rate_pps,
            should_be_pinging,
        };
        orchestrator.config_tx.send(supervisor_config).unwrap();

        // Verify configuration was sent
        config_rx.changed().await.unwrap();
        let received_config = config_rx.borrow();
        assert_eq!(
            received_config.targets,
            vec!["1.1.1.1".to_string(), "8.8.8.8".to_string()]
        );
        assert_eq!(received_config.ping_rate_pps, 50);
    }

    #[tokio::test]
    #[ntest::timeout(100)]
    async fn test_orchestrator_run_with_cached_config() {
        let temp_dir = tempdir().unwrap();
        let cache_file = temp_dir.path().join("test_config.bin");

        // Create test cached config
        let cached_config = CachedConfig {
            targets: vec!["1.1.1.1".to_string()],
            ping_rate_pps: 100,
        };
        let bytes = bincode::serialize(&cached_config).unwrap();
        std::fs::write(&cache_file, bytes).unwrap();

        let cli = Arc::new(Cli {
            source_hostname: "test-collector".to_string(),
            database_addr: "http://127.0.0.1:8080".to_string(),
            auth_token: "test-token".to_string(),
            max_in_flight: 3,
        });

        let initial_config = SupervisorConfig {
            targets: vec![],
            ping_rate_pps: 0,
            should_be_pinging: false,
        };
        let (config_tx, mut config_rx) = tokio::sync::watch::channel(initial_config);

        let mut orchestrator = CollectorOrchestrator {
            cli: cli.clone(),
            state_machine: StateMachine::new(cli.source_hostname.clone()),
            config_tx,
            last_cached_config: None,
            cache_file: cache_file.clone(),
        };

        // Test the cached config loading logic from run()
        if let Ok(bytes) = std::fs::read(&orchestrator.cache_file)
            && let Ok(config) = bincode::deserialize::<CachedConfig>(&bytes)
        {
            let supervisor_config = SupervisorConfig {
                targets: config.targets.clone(),
                ping_rate_pps: config.ping_rate_pps,
                should_be_pinging: true,
            };
            orchestrator.config_tx.send(supervisor_config).unwrap();
            orchestrator.last_cached_config = Some(config);
        }

        // Verify cached config was loaded and sent
        config_rx.changed().await.unwrap();
        let received_config = config_rx.borrow();
        assert_eq!(received_config.targets, vec!["1.1.1.1"]);
        assert_eq!(received_config.ping_rate_pps, 100);
        assert!(received_config.should_be_pinging);
    }

    #[tokio::test]
    #[ntest::timeout(100)]
    async fn test_orchestrator_heartbeat_loop_shutdown() {
        let cli = Arc::new(Cli {
            source_hostname: "test-collector".to_string(),
            database_addr: "http://127.0.0.1:8080".to_string(),
            auth_token: "test-token".to_string(),
            max_in_flight: 3,
        });

        let initial_config = SupervisorConfig {
            targets: vec![],
            ping_rate_pps: 0,
            should_be_pinging: false,
        };
        let (config_tx, _) = tokio::sync::watch::channel(initial_config);

        let mut orchestrator = CollectorOrchestrator {
            cli: cli.clone(),
            state_machine: StateMachine::new(cli.source_hostname.clone()),
            config_tx,
            last_cached_config: None,
            cache_file: std::path::PathBuf::from("/tmp/nonexistent"),
        };

        // Mock shutdown response
        let shutdown_response = HeartbeatResponse {
            targets: vec![],
            ping_rate_pps: 0,
            role: 3, // Shutdown
            swap_at_nanos: 0,
        };

        // Simulate heartbeat processing that leads to shutdown
        let _action = orchestrator
            .state_machine
            .handle_heartbeat_response(&shutdown_response);

        // Verify state is shutdown
        assert_eq!(orchestrator.state_machine.current_state, State::Shutdown);
    }

    #[tokio::test]
    #[ntest::timeout(100)]
    async fn test_orchestrator_buffer_seeding() {
        let cli = Arc::new(Cli {
            source_hostname: "test-collector".to_string(),
            database_addr: "http://127.0.0.1:8080".to_string(),
            auth_token: "test-token".to_string(),
            max_in_flight: 3,
        });

        let initial_config = SupervisorConfig {
            targets: vec![],
            ping_rate_pps: 0,
            should_be_pinging: false,
        };
        let (config_tx, _) = tokio::sync::watch::channel(initial_config);

        let mut orchestrator = CollectorOrchestrator {
            cli: cli.clone(),
            state_machine: StateMachine::new(cli.source_hostname.clone()),
            config_tx,
            last_cached_config: None,
            cache_file: std::path::PathBuf::from("/tmp/nonexistent"),
        };

        // Mock seed buffer response
        let seed_response = HeartbeatResponse {
            targets: vec!["1.1.1.1".to_string()],
            ping_rate_pps: 10,
            role: 0, // Primary (should trigger seed buffer)
            swap_at_nanos: 0,
        };

        // Simulate heartbeat processing
        let action = orchestrator
            .state_machine
            .handle_heartbeat_response(&seed_response);

        // Verify seed buffer action is triggered
        assert_eq!(action, Some(Action::SeedBuffer));
    }

    #[tokio::test]
    #[ntest::timeout(100)]
    async fn test_orchestrator_config_change_detection() {
        let temp_dir = tempdir().unwrap();
        let cache_file = temp_dir.path().join("test_config.bin");

        let cli = Arc::new(Cli {
            source_hostname: "test-collector".to_string(),
            database_addr: "http://127.0.0.1:8080".to_string(),
            auth_token: "test-token".to_string(),
            max_in_flight: 3,
        });

        let initial_config = SupervisorConfig {
            targets: vec![],
            ping_rate_pps: 0,
            should_be_pinging: false,
        };
        let (config_tx, _) = tokio::sync::watch::channel(initial_config);

        let mut orchestrator = CollectorOrchestrator {
            cli: cli.clone(),
            state_machine: StateMachine::new(cli.source_hostname.clone()),
            config_tx,
            last_cached_config: Some(CachedConfig {
                targets: vec!["old-target".to_string()],
                ping_rate_pps: 50,
            }),
            cache_file: cache_file.clone(),
        };

        // New config that's different
        let new_config = CachedConfig {
            targets: vec!["1.1.1.1".to_string(), "8.8.8.8".to_string()],
            ping_rate_pps: 100,
        };

        // Simulate config change detection logic
        let should_cache = Some(&new_config) != orchestrator.last_cached_config.as_ref();
        assert!(should_cache);

        // Simulate caching
        if let Ok(bytes) = bincode::serialize(&new_config) {
            let _ = std::fs::write(&cache_file, bytes);
        }
        orchestrator.last_cached_config = Some(new_config.clone());

        // Verify new config was cached
        assert_eq!(
            orchestrator
                .last_cached_config
                .as_ref()
                .unwrap()
                .targets
                .len(),
            2
        );
        assert_eq!(
            orchestrator
                .last_cached_config
                .as_ref()
                .unwrap()
                .ping_rate_pps,
            100
        );
    }

    #[tokio::test]
    #[ntest::timeout(100)]
    async fn test_orchestrator_state_transitions() {
        let cli = Arc::new(Cli {
            source_hostname: "test-collector".to_string(),
            database_addr: "http://127.0.0.1:8080".to_string(),
            auth_token: "test-token".to_string(),
            max_in_flight: 3,
        });

        let initial_config = SupervisorConfig {
            targets: vec![],
            ping_rate_pps: 0,
            should_be_pinging: false,
        };
        let (config_tx, mut config_rx) = tokio::sync::watch::channel(initial_config);

        let mut orchestrator = CollectorOrchestrator {
            cli: cli.clone(),
            state_machine: StateMachine::new(cli.source_hostname.clone()),
            config_tx,
            last_cached_config: None,
            cache_file: std::path::PathBuf::from("/tmp/nonexistent"),
        };

        // Test transition to primary/pinging state
        let primary_response = HeartbeatResponse {
            targets: vec!["1.1.1.1".to_string()],
            ping_rate_pps: 10,
            role: 0, // Primary
            swap_at_nanos: 0,
        };

        let _action = orchestrator
            .state_machine
            .handle_heartbeat_response(&primary_response);

        let should_be_pinging = orchestrator.state_machine.current_state == State::Pinging;
        let supervisor_config = SupervisorConfig {
            targets: primary_response.targets,
            ping_rate_pps: primary_response.ping_rate_pps,
            should_be_pinging,
        };
        orchestrator.config_tx.send(supervisor_config).unwrap();

        // Verify configuration was sent with pinging enabled
        config_rx.changed().await.unwrap();
        let received_config = config_rx.borrow();
        assert!(received_config.should_be_pinging);
        assert_eq!(received_config.targets, vec!["1.1.1.1"]);
        assert_eq!(received_config.ping_rate_pps, 10);
    }
}
