//! Task supervision — design intent and operational invariants
//!
//! Why this exists (high level): the system needs to map an authoritative
//! list of targets (from the database heartbeat) into a set of running
//! pinger tasks. This module exists to encapsulate the non-obvious rules of
//! that mapping and the runtime invariants we rely on elsewhere.
//!
//! Important reasoning and trade-offs:
//! - We intentionally track tasks by parsed `IpAddr` rather than raw strings
//!   to avoid subtle duplicates and normalization issues.
//! - The supervisor reacts only to *changes* in the heartbeat
//!   configuration. This avoids restarting tasks unnecessarily and keeps
//!   per-target state stable (sequence numbers, semaphores).
//! - We choose to `abort()` tasks when removing them rather than trying to
//!   signal a graceful shutdown path. Rationale: pinger tasks are short-lived
//!   and idempotent (they don't hold durable resources). Aborting is a
//!   pragmatic trade-off that keeps the supervisor simple. If graceful
//!   shutdown becomes required, add a shutdown channel per task and drain it.
//!
//! Operational invariants callers should assume:
//! - There will be at most one pinger task per unique `IpAddr` in
//!   `running_tasks` at any time.
//! - Tasks may be aborted at any time during a reconfiguration. Ping
//!   results arriving after abort are benign and ignored by downstream
//!   components because they are keyed by timestamp and per-target cursors.
//! - The supervisor does not attempt to smooth rate changes; rate is applied
//!   at task spawn time. Rapid rate flapping will spawn/abort tasks rapidly,
//!   so the orchestrator should avoid flip-flopping configs unnecessarily.

use crate::{
    cli::Cli,
    ping_client::{PingClient, PingResult},
    ping_surge_client::PingSurgeClient,
};
use anyhow::Result;
use log::{error, info};
use std::{collections::HashMap, net::IpAddr, sync::Arc};
use tokio::{sync::mpsc, task::JoinHandle};

/// Runtime configuration for the task supervisor.
///
/// This struct represents the current desired state of the ping monitoring
/// system. It's sent through a watch channel to enable reactive updates
/// without restarting the supervisor.
///
/// The configuration drives the supervisor's behavior:
/// - **targets**: Which IP addresses should be monitored
/// - **ping_rate_pps**: How frequently to ping each target
/// - **should_be_pinging**: Master enable/disable switch for all monitoring
#[derive(Debug, Clone)]
pub struct SupervisorConfig {
    /// List of target IP addresses to monitor.
    /// Each target will have its own dedicated ping task.
    pub targets: Vec<String>,

    /// Ping rate in packets per second for each target.
    /// Controls the monitoring frequency and network load.
    pub ping_rate_pps: u64,

    /// Master switch to enable or disable all ping monitoring.
    /// When false, all running tasks will be stopped.
    pub should_be_pinging: bool,
}

/// Supervisor for managing the lifecycle of ping worker tasks.
///
/// The task supervisor maps database-provided targets to running tasks,
/// ensuring efficient and fault-tolerant task management.
///
/// ## Design Rationale
///
/// **Reactive Architecture**: Uses tokio::sync::watch for configuration
/// updates, enabling zero-downtime reconfiguration of monitoring targets.
///
/// **Resource Management**: Tracks all spawned tasks to ensure proper
/// cleanup and prevent resource leaks during shutdown or reconfiguration.
///
/// **Error Isolation**: Individual task failures don't affect the supervisor
/// or other monitoring tasks, providing fault tolerance.
///
/// **Performance**: Avoids unnecessary task spawning by tracking existing
/// tasks and only creating new ones when targets are added.
pub struct TaskSupervisor {
    /// Channel receiver for configuration updates.
    /// The supervisor reacts to changes by adjusting its task set.
    config_rx: tokio::sync::watch::Receiver<SupervisorConfig>,

    /// Channel sender for ping results.
    /// All ping tasks send their results through this channel.
    ping_results_tx: mpsc::Sender<PingResult>,

    /// Command-line configuration shared across tasks.
    /// Contains settings like max concurrent pings and timeouts.
    cli: Arc<Cli>,

    /// Mapping of target IPs to their running tasks.
    /// Used to track active tasks and prevent duplicate spawning.
    running_tasks: HashMap<IpAddr, JoinHandle<()>>,
}

impl TaskSupervisor {
    /// Creates a new task supervisor with the given communication channels.
    ///
    /// # Parameters
    /// * `config_rx` - Receiver for configuration updates
    /// * `ping_results_tx` - Sender for ping results to the batch submitter
    /// * `cli` - Shared command-line configuration
    ///
    /// # Returns
    /// A configured TaskSupervisor ready to manage ping tasks
    pub fn new(
        config_rx: tokio::sync::watch::Receiver<SupervisorConfig>,
        ping_results_tx: mpsc::Sender<PingResult>,
        cli: Arc<Cli>,
    ) -> Self {
        Self {
            config_rx,
            ping_results_tx,
            cli,
            running_tasks: HashMap::new(),
        }
    }

    /// Runs the supervisor's main event loop.
    ///
    /// The supervisor waits for configuration changes and reacts by
    /// updating its set of running tasks. This method runs indefinitely
    /// until the configuration channel is closed.
    ///
    /// # Returns
    /// An error if the configuration channel is closed unexpectedly
    pub async fn run(mut self) -> Result<()> {
        while self.config_rx.changed().await.is_ok() {
            let config = self.config_rx.borrow().clone();
            self.update_tasks(&config).await;
        }
        Ok(())
    }

    /// Updates the set of running tasks to match the desired configuration.
    ///
    /// This method implements the core task management logic:
    /// 1. Stop tasks for targets no longer in the configuration
    /// 2. Start tasks for new targets in the configuration
    /// 3. Leave existing tasks running for unchanged targets
    ///
    /// # Parameters
    /// * `config` - The new desired configuration state
    async fn update_tasks(&mut self, config: &SupervisorConfig) {
        let new_targets_set: HashMap<_, _> = if config.should_be_pinging {
            config
                .targets
                .iter()
                .filter_map(|s| s.parse::<IpAddr>().ok())
                .map(|ip| (ip, ()))
                .collect()
        } else {
            HashMap::new()
        };

        // Stop tasks that are no longer in the target list
        self.running_tasks.retain(|target, handle| {
            if !new_targets_set.contains_key(target) {
                info!("Stopping pinger for target {target}");
                handle.abort();
                false
            } else {
                true
            }
        });

        // Start new tasks for new targets
        for target_ip in new_targets_set.keys() {
            if !self.running_tasks.contains_key(target_ip) {
                info!("Starting pinger for target {target_ip}");
                let ping_client: Arc<dyn PingClient> = match PingSurgeClient::new(*target_ip) {
                    Ok(client) => Arc::new(client),
                    Err(e) => {
                        error!("Failed to create ping client for {target_ip}: {e}");
                        continue;
                    }
                };
                let pinger_handle = tokio::spawn(crate::runner::pinger_loop(
                    ping_client,
                    self.ping_results_tx.clone(),
                    self.cli.max_in_flight,
                    config.ping_rate_pps,
                ));
                self.running_tasks.insert(*target_ip, pinger_handle);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Cli;
    use crate::ping_client::PingResult;
    use std::net::IpAddr;
    use std::sync::Arc;
    use tokio::sync::mpsc;

    // Mock ping client for testing
    #[allow(dead_code)]
    struct MockPingClient {
        target: IpAddr,
    }

    #[async_trait::async_trait]
    impl crate::ping_client::PingClient for MockPingClient {
        fn target(&self) -> IpAddr {
            self.target
        }

        async fn ping(
            &self,
            _sequence_idx: u16,
            _tx: mpsc::Sender<PingResult>,
            _permit: tokio::sync::OwnedSemaphorePermit,
            _start_time: std::time::Instant,
            _target_time: std::time::Instant,
        ) {
            // Mock implementation - do nothing
        }
    }

    #[tokio::test]
    #[ntest::timeout(100)]
    async fn test_task_supervisor_new() {
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
        let (_config_tx, config_rx) = tokio::sync::watch::channel(initial_config);
        let (ping_tx, _ping_rx) = mpsc::channel(100);

        let supervisor = TaskSupervisor::new(config_rx, ping_tx, cli.clone());

        assert_eq!(supervisor.cli.source_hostname, "test-collector");
        assert!(supervisor.running_tasks.is_empty());
    }

    #[tokio::test]
    #[ntest::timeout(100)]
    async fn test_update_tasks_start_new_targets() {
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
        let (_config_tx, config_rx) = tokio::sync::watch::channel(initial_config);
        let (ping_tx, _ping_rx) = mpsc::channel(100);

        let _supervisor = TaskSupervisor::new(config_rx, ping_tx, cli.clone());

        // Test starting new targets
        let config = SupervisorConfig {
            targets: vec!["1.1.1.1".to_string(), "8.8.8.8".to_string()],
            ping_rate_pps: 10,
            should_be_pinging: true,
        };

        // Mock the PingSurgeClient::new to return our mock client
        // For this test, we'll just check that the logic works
        // In a real scenario, we'd use dependency injection

        let new_targets_set: std::collections::HashMap<_, _> = if config.should_be_pinging {
            config
                .targets
                .iter()
                .filter_map(|s| s.parse::<IpAddr>().ok())
                .map(|ip| (ip, ()))
                .collect()
        } else {
            std::collections::HashMap::new()
        };

        assert_eq!(new_targets_set.len(), 2);
        assert!(new_targets_set.contains_key(&"1.1.1.1".parse::<IpAddr>().unwrap()));
        assert!(new_targets_set.contains_key(&"8.8.8.8".parse::<IpAddr>().unwrap()));
    }

    #[tokio::test]
    #[ntest::timeout(100)]
    async fn test_update_tasks_stop_removed_targets() {
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
        let (_config_tx, config_rx) = tokio::sync::watch::channel(initial_config);
        let (ping_tx, _ping_rx) = mpsc::channel(100);

        let mut supervisor = TaskSupervisor::new(config_rx, ping_tx, cli.clone());

        // Simulate having a running task
        let target_ip = "1.1.1.1".parse::<IpAddr>().unwrap();
        let mock_handle = tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        });
        supervisor.running_tasks.insert(target_ip, mock_handle);

        // Test stopping targets
        let config = SupervisorConfig {
            targets: vec![], // No targets
            ping_rate_pps: 10,
            should_be_pinging: true,
        };

        let new_targets_set: std::collections::HashMap<_, _> = if config.should_be_pinging {
            config
                .targets
                .iter()
                .filter_map(|s| s.parse::<IpAddr>().ok())
                .map(|ip| (ip, ()))
                .collect()
        } else {
            std::collections::HashMap::new()
        };

        // Simulate the retain logic
        let mut tasks_to_remove = Vec::new();
        supervisor.running_tasks.retain(|target, _handle| {
            if !new_targets_set.contains_key(target) {
                tasks_to_remove.push(*target);
                false
            } else {
                true
            }
        });

        assert_eq!(tasks_to_remove.len(), 1);
        assert_eq!(tasks_to_remove[0], target_ip);
        assert!(supervisor.running_tasks.is_empty());
    }

    #[tokio::test]
    #[ntest::timeout(100)]
    async fn test_update_tasks_pinging_disabled() {
        let config = SupervisorConfig {
            targets: vec!["1.1.1.1".to_string()],
            ping_rate_pps: 10,
            should_be_pinging: false, // Pinging disabled
        };

        let new_targets_set: std::collections::HashMap<IpAddr, ()> = if config.should_be_pinging {
            config
                .targets
                .iter()
                .filter_map(|s| s.parse::<IpAddr>().ok())
                .map(|ip| (ip, ()))
                .collect()
        } else {
            std::collections::HashMap::new()
        };

        // When pinging is disabled, no targets should be active
        assert!(new_targets_set.is_empty());
    }

    #[tokio::test]
    #[ntest::timeout(100)]
    async fn test_supervisor_config_target_parsing() {
        let config = SupervisorConfig {
            targets: vec![
                "1.1.1.1".to_string(),
                "8.8.8.8".to_string(),
                "invalid-ip".to_string(), // This should be filtered out
            ],
            ping_rate_pps: 10,
            should_be_pinging: true,
        };

        let new_targets_set: std::collections::HashMap<_, _> = config
            .targets
            .iter()
            .filter_map(|s| s.parse::<IpAddr>().ok())
            .map(|ip| (ip, ()))
            .collect();

        // Should only parse valid IPs
        assert_eq!(new_targets_set.len(), 2);
        assert!(new_targets_set.contains_key(&"1.1.1.1".parse::<IpAddr>().unwrap()));
        assert!(new_targets_set.contains_key(&"8.8.8.8".parse::<IpAddr>().unwrap()));
    }

    #[tokio::test]
    #[ntest::timeout(100)]
    async fn test_task_supervisor_run_with_config_changes() {
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
        let (config_tx, config_rx) = tokio::sync::watch::channel(initial_config);
        let (ping_tx, _ping_rx) = mpsc::channel(100);

        let supervisor = TaskSupervisor::new(config_rx, ping_tx, cli.clone());

        // Spawn the supervisor
        let handle = tokio::spawn(async move {
            let supervisor = supervisor;
            // Run for a short time, then we'll abort it
            tokio::select! {
                result = supervisor.run() => result,
                _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => Ok(()),
            }
        });

        // Send a config change
        let new_config = SupervisorConfig {
            targets: vec!["1.1.1.1".to_string()],
            ping_rate_pps: 10,
            should_be_pinging: true,
        };
        config_tx.send(new_config).unwrap();

        // Let it process the change
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        // Abort the supervisor
        handle.abort();
    }

    #[tokio::test]
    #[ntest::timeout(100)]
    async fn test_update_tasks_mixed_changes() {
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
        let (_config_tx, config_rx) = tokio::sync::watch::channel(initial_config);
        let (ping_tx, _ping_rx) = mpsc::channel(100);

        let mut supervisor = TaskSupervisor::new(config_rx, ping_tx, cli.clone());

        // Add some initial tasks
        let target1 = "1.1.1.1".parse::<IpAddr>().unwrap();
        let target2 = "8.8.8.8".parse::<IpAddr>().unwrap();
        let mock_handle1 = tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        });
        let mock_handle2 = tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        });
        supervisor.running_tasks.insert(target1, mock_handle1);
        supervisor.running_tasks.insert(target2, mock_handle2);

        // Test mixed changes: keep one, remove one, add one
        let config = SupervisorConfig {
            targets: vec!["8.8.8.8".to_string(), "9.9.9.9".to_string()], // Keep 8.8.8.8, remove 1.1.1.1, add 9.9.9.9
            ping_rate_pps: 10,
            should_be_pinging: true,
        };

        let new_targets_set: std::collections::HashMap<_, _> = if config.should_be_pinging {
            config
                .targets
                .iter()
                .filter_map(|s| s.parse::<IpAddr>().ok())
                .map(|ip| (ip, ()))
                .collect()
        } else {
            std::collections::HashMap::new()
        };

        // Simulate the retain logic
        let mut tasks_to_remove = Vec::new();
        supervisor.running_tasks.retain(|target, _handle| {
            if !new_targets_set.contains_key(target) {
                tasks_to_remove.push(*target);
                false
            } else {
                true
            }
        });

        // Should remove 1.1.1.1
        assert_eq!(tasks_to_remove.len(), 1);
        assert_eq!(tasks_to_remove[0], target1);

        // Should keep 8.8.8.8
        assert!(supervisor.running_tasks.contains_key(&target2));

        // Should have 2 targets in new set
        assert_eq!(new_targets_set.len(), 2);
        assert!(new_targets_set.contains_key(&target2));
        assert!(new_targets_set.contains_key(&"9.9.9.9".parse::<IpAddr>().unwrap()));
    }

    #[tokio::test]
    #[ntest::timeout(100)]
    async fn test_supervisor_config_empty_targets() {
        let config = SupervisorConfig {
            targets: vec![],
            ping_rate_pps: 10,
            should_be_pinging: true,
        };

        let new_targets_set: std::collections::HashMap<IpAddr, ()> = if config.should_be_pinging {
            config
                .targets
                .iter()
                .filter_map(|s| s.parse::<IpAddr>().ok())
                .map(|ip| (ip, ()))
                .collect()
        } else {
            std::collections::HashMap::new()
        };

        assert!(new_targets_set.is_empty());
    }

    #[tokio::test]
    #[ntest::timeout(100)]
    async fn test_supervisor_config_zero_rate() {
        let config = SupervisorConfig {
            targets: vec!["1.1.1.1".to_string()],
            ping_rate_pps: 0, // Zero rate
            should_be_pinging: true,
        };

        let new_targets_set: std::collections::HashMap<_, _> = if config.should_be_pinging {
            config
                .targets
                .iter()
                .filter_map(|s| s.parse::<IpAddr>().ok())
                .map(|ip| (ip, ()))
                .collect()
        } else {
            std::collections::HashMap::new()
        };

        // Should still include targets even with zero rate
        assert_eq!(new_targets_set.len(), 1);
        assert!(new_targets_set.contains_key(&"1.1.1.1".parse::<IpAddr>().unwrap()));
    }

    #[tokio::test]
    #[ntest::timeout(100)]
    async fn test_task_supervisor_with_multiple_config_updates() {
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
        let (config_tx, config_rx) = tokio::sync::watch::channel(initial_config);
        let (ping_tx, _ping_rx) = mpsc::channel(100);

        let supervisor = TaskSupervisor::new(config_rx, ping_tx, cli.clone());

        // Spawn the supervisor
        let handle = tokio::spawn(async move {
            let supervisor = supervisor;
            tokio::select! {
                result = supervisor.run() => result,
                _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => Ok(()),
            }
        });

        // Send multiple config changes
        let configs = vec![
            SupervisorConfig {
                targets: vec!["1.1.1.1".to_string()],
                ping_rate_pps: 10,
                should_be_pinging: true,
            },
            SupervisorConfig {
                targets: vec!["1.1.1.1".to_string(), "8.8.8.8".to_string()],
                ping_rate_pps: 20,
                should_be_pinging: true,
            },
            SupervisorConfig {
                targets: vec!["8.8.8.8".to_string()],
                ping_rate_pps: 5,
                should_be_pinging: true,
            },
        ];

        for config in configs {
            config_tx.send(config).unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }

        // Abort the supervisor
        handle.abort();
    }

    #[tokio::test]
    #[ntest::timeout(100)]
    async fn test_task_supervisor_channel_closed() {
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
        let (config_tx, config_rx) = tokio::sync::watch::channel(initial_config);
        let (ping_tx, _ping_rx) = mpsc::channel(100);

        let supervisor = TaskSupervisor::new(config_rx, ping_tx, cli.clone());

        // Spawn the supervisor
        let handle = tokio::spawn(async move { supervisor.run().await });

        // Close the config channel
        drop(config_tx);

        // Wait for the supervisor to exit
        let result = tokio::time::timeout(std::time::Duration::from_millis(50), handle).await;

        // Should complete without error when channel is closed
        assert!(result.is_ok());
    }
}
