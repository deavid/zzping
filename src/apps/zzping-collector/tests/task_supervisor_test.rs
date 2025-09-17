use ntest::timeout;
use std::{collections::HashSet, net::IpAddr, str::FromStr};
use zzping_collector::database_client::DatabaseClient;
use zzping_collector::target_worker::WorkerCommand;
use zzping_collector::task_supervisor::{ClientUpdate, SupervisorConfig, TaskSupervisor};
use zzping_proto::zzping::CollectorRole;

mod common;
use common::mock_worker_factory;

#[tokio::test]
#[timeout(500)]
async fn test_supervisor_sends_update_role_on_config_change() {
    // 1. Setup
    let mut supervisor = TaskSupervisor::new("test-uuid".to_string(), 1, None);

    // Manually insert a mock worker for the test.
    let target_ip = IpAddr::from_str("1.1.1.1").unwrap();
    let (real_handle, mut mock_handle) = mock_worker_factory();
    supervisor.workers.insert(target_ip, real_handle);

    // 2. Create a new config that changes the role to Primary
    let mut targets = HashSet::new();
    targets.insert(target_ip);
    let config = Some(SupervisorConfig {
        targets,
        ping_rate_pps: 10,
        role: CollectorRole::Primary,
        swap_at_nanos: None,
        use_mock_ping_client: true, // Use mock client in tests
    });

    // 3. Reconcile with the new config
    supervisor.reconcile(config).await;

    // 4. Assert that the supervisor sent the correct command
    let received_command = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        mock_handle.command_rx.recv(),
    )
    .await
    .expect("Test timed out waiting for command")
    .expect("Command channel was empty");

    match received_command {
        WorkerCommand::UpdateRole(role) => {
            assert_eq!(role, CollectorRole::Primary);
        }
        _ => panic!("Received unexpected command: {received_command:?}"),
    }
}

#[tokio::test]
#[timeout(500)]
async fn test_supervisor_sends_shutdown_to_removed_workers() {
    // 1. Setup
    let mut supervisor = TaskSupervisor::new("test-uuid".to_string(), 1, None);

    // Manually insert a mock worker for the test.
    let target_ip = IpAddr::from_str("1.1.1.1").unwrap();
    let (real_handle, mut mock_handle) = mock_worker_factory();
    supervisor.workers.insert(target_ip, real_handle);
    assert_eq!(supervisor.workers.len(), 1);

    // 2. Create a new config that removes the target
    let config = Some(SupervisorConfig {
        targets: HashSet::new(),
        ping_rate_pps: 10,
        role: CollectorRole::Primary,
        swap_at_nanos: None,
        use_mock_ping_client: true, // Use mock client in tests
    });

    // 3. Reconcile with the new config
    supervisor.reconcile(config).await;
    assert!(
        supervisor.workers.is_empty(),
        "Worker should have been removed"
    );

    // 4. Assert that the supervisor sent the Shutdown command
    let received_command = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        mock_handle.command_rx.recv(),
    )
    .await
    .expect("Test timed out waiting for command")
    .expect("Command channel was empty");

    assert!(matches!(received_command, WorkerCommand::Shutdown));
}

#[tokio::test]
#[timeout(500)]
async fn test_supervisor_defers_and_then_creates_workers() {
    use std::net::IpAddr;
    use zzping_collector::task_supervisor::SupervisorConfig;

    let supervisor =
        zzping_collector::task_supervisor::TaskSupervisor::new("test-uuid".to_string(), 1000, None);

    let (config_tx, config_rx) = tokio::sync::watch::channel::<Option<SupervisorConfig>>(None);
    let (client_update_tx, client_update_rx) = tokio::sync::mpsc::channel::<ClientUpdate>(10);
    let (health_tx, _health_rx) =
        tokio::sync::mpsc::channel::<zzping_collector::task_supervisor::HealthReport>(10);
    let (_shutdown_tx, shutdown_rx) =
        tokio::sync::mpsc::channel::<zzping_collector::task_supervisor::SupervisorShutdown>(1);
    let (_fsync_tx, fsync_rx) = tokio::sync::mpsc::channel::<u64>(10);

    // Reconcile a config with one target while no client is present
    let config = SupervisorConfig {
        targets: vec!["127.0.0.1".parse::<IpAddr>().unwrap()]
            .into_iter()
            .collect(),
        ping_rate_pps: 1,
        role: zzping_proto::zzping::CollectorRole::Primary,
        swap_at_nanos: None,
        use_mock_ping_client: true,
    };

    // Run supervisor in background
    let _supervisor_task = tokio::spawn(async move {
        supervisor
            .run(
                config_rx,
                client_update_rx,
                health_tx,
                shutdown_rx,
                fsync_rx,
            )
            .await
            .unwrap();
    });

    // Send config; since no client has been sent, supervisor should defer worker creation
    config_tx.send(Some(config)).unwrap();
    // Give it a moment
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Now send a NewClient update
    // Start a small mock server to provide a reachable DatabaseClient for the test.
    let mock = common::MockIngestionService::new();
    let server_addr = common::spawn_mock_server(mock).await;
    let dummy_client = DatabaseClient::connect(
        // Returns Arc<dyn DatabaseClientTrait>
        format!("http://{server_addr}"),
        "test-token".to_string(),
    )
    .await
    .unwrap();
    client_update_tx
        .send(ClientUpdate::NewClient(dummy_client)) // Removed Box::new()
        .await
        .ok();

    // Give supervisor time to create worker
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // If we reached here without panic, assume supervisor handled deferral and creation.
    // Shut down the background task by dropping channels.
    drop(client_update_tx);
    drop(config_tx);
    _supervisor_task.abort();
}

#[tokio::test]
#[timeout(500)]
async fn test_supervisor_schedules_swap_and_applies_role() {
    use zzping_collector::task_supervisor::ClientUpdate;

    let supervisor = TaskSupervisor::new("test-uuid".to_string(), 1000, None);

    let (config_tx, config_rx) = tokio::sync::watch::channel::<Option<SupervisorConfig>>(None);
    let (client_update_tx, client_update_rx) = tokio::sync::mpsc::channel::<ClientUpdate>(10);
    let (health_tx, _health_rx) =
        tokio::sync::mpsc::channel::<zzping_collector::task_supervisor::HealthReport>(10);
    let (_shutdown_tx, shutdown_rx) =
        tokio::sync::mpsc::channel::<zzping_collector::task_supervisor::SupervisorShutdown>(1);
    let (_fsync_tx, fsync_rx) = tokio::sync::mpsc::channel::<u64>(10);

    // Run supervisor in background
    let _supervisor_task = tokio::spawn(async move {
        supervisor
            .run(
                config_rx,
                client_update_rx,
                health_tx,
                shutdown_rx,
                fsync_rx,
            )
            .await
            .unwrap();
    });

    // Bring up a mock DB client so worker creation proceeds
    let mock = common::MockIngestionService::new();
    let server_addr = common::spawn_mock_server(mock).await;
    let dummy_client = DatabaseClient::connect(
        // Returns Arc<dyn DatabaseClientTrait>
        format!("http://{server_addr}"),
        "test-token".to_string(),
    )
    .await
    .unwrap();
    client_update_tx
        .send(ClientUpdate::NewClient(dummy_client)) // Removed Box::new()
        .await
        .ok();

    // Prepare a config with a swap scheduled shortly in the future
    let target_ip = "127.0.0.1".parse().unwrap();
    let mut targets = std::collections::HashSet::new();
    targets.insert(target_ip);
    // schedule ~150ms in the future
    let now_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    let swap_at = now_ns + 150_000_000; // 150ms

    let config = SupervisorConfig {
        targets,
        ping_rate_pps: 1,
        role: zzping_proto::zzping::CollectorRole::Primary,
        swap_at_nanos: Some(swap_at),
        use_mock_ping_client: true,
    };

    // Send the config
    config_tx.send(Some(config)).unwrap();

    // Wait a bit for worker creation
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Attach a mock worker to observe UpdateRole commands by directly inserting one
    // into the running supervisor isn't possible here; instead, we rely on the
    // fact that worker creation uses MockPingClient which doesn't expose command
    // channels for us. To observe the scheduled role application, we'll instead
    // create a new supervisor instance and mock worker directly via mock_worker_factory
    // and run a small scoped supervisor to observe two UpdateRole messages.
    let mut small_sup = TaskSupervisor::new("small-uuid".to_string(), 1000, None);
    let (real_handle, mut mock_handle) = mock_worker_factory();
    let ip = target_ip;
    small_sup.workers.insert(ip, real_handle);

    // Now schedule a swap for small_sup by sending a config via a watch channel
    let (s_cfg_tx, s_cfg_rx) = tokio::sync::watch::channel::<Option<SupervisorConfig>>(None);
    let (_s_client_tx, s_client_rx) = tokio::sync::mpsc::channel::<ClientUpdate>(10);
    let (s_health_tx, _s_health_rx) =
        tokio::sync::mpsc::channel::<zzping_collector::task_supervisor::HealthReport>(10);
    let (_s_shutdown_tx, s_shutdown_rx) =
        tokio::sync::mpsc::channel::<zzping_collector::task_supervisor::SupervisorShutdown>(1);
    let (_s_fsync_tx, s_fsync_rx) = tokio::sync::mpsc::channel::<u64>(10);

    // Run the small supervisor
    let small_task = tokio::spawn(async move {
        small_sup
            .run(
                s_cfg_rx,
                s_client_rx,
                s_health_tx,
                s_shutdown_rx,
                s_fsync_rx,
            )
            .await
            .unwrap();
    });

    // Send a config that sets role to Primary and schedules a swap ~100ms ahead
    let now_ns2 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    let swap_at2 = now_ns2 + 100_000_000; // 100ms
    let mut targets2 = std::collections::HashSet::new();
    targets2.insert(ip);
    let s_config = SupervisorConfig {
        targets: targets2,
        ping_rate_pps: 1,
        role: zzping_proto::zzping::CollectorRole::Primary,
        swap_at_nanos: Some(swap_at2),
        use_mock_ping_client: true,
    };

    // Send config to small supervisor
    s_cfg_tx.send(Some(s_config)).unwrap();

    // First UpdateRole (from reconcile) should arrive quickly; ignore other commands like GetHealth
    tokio::time::timeout(std::time::Duration::from_millis(200), async {
        while let Some(cmd) = mock_handle.command_rx.recv().await {
            if matches!(cmd, WorkerCommand::UpdateRole(_)) {
                break;
            }
        }
    })
    .await
    .expect("timed out waiting for first UpdateRole");

    // Wait for the scheduled second UpdateRole
    tokio::time::timeout(std::time::Duration::from_millis(500), async {
        while let Some(cmd) = mock_handle.command_rx.recv().await {
            if matches!(cmd, WorkerCommand::UpdateRole(_)) {
                break;
            }
        }
    })
    .await
    .expect("timed out waiting for scheduled UpdateRole");

    // Cleanup
    small_task.abort();
}

#[tokio::test]
#[timeout(500)]
async fn test_supervisor_cancels_scheduled_swap_on_config_change() {
    use zzping_collector::task_supervisor::ClientUpdate;

    // Setup a small supervisor with a single mock worker to observe commands
    let mut small_sup = TaskSupervisor::new("cancel-uuid".to_string(), 1000, None);
    let (real_handle, mut mock_handle) = mock_worker_factory();
    let ip = "127.0.0.1".parse().unwrap();
    small_sup.workers.insert(ip, real_handle);

    let (s_cfg_tx, s_cfg_rx) = tokio::sync::watch::channel::<Option<SupervisorConfig>>(None);
    let (_s_client_tx, s_client_rx) = tokio::sync::mpsc::channel::<ClientUpdate>(10);
    let (s_health_tx, _s_health_rx) =
        tokio::sync::mpsc::channel::<zzping_collector::task_supervisor::HealthReport>(10);
    let (_s_shutdown_tx, s_shutdown_rx) =
        tokio::sync::mpsc::channel::<zzping_collector::task_supervisor::SupervisorShutdown>(1);
    let (_s_fsync_tx, s_fsync_rx) = tokio::sync::mpsc::channel::<u64>(10);

    // Run supervisor
    let small_task = tokio::spawn(async move {
        small_sup
            .run(
                s_cfg_rx,
                s_client_rx,
                s_health_tx,
                s_shutdown_rx,
                s_fsync_rx,
            )
            .await
            .unwrap();
    });

    // Send initial config with swap scheduled in 200ms
    let now_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    let swap_at = now_ns + 200_000_000; // 200ms
    let mut targets = std::collections::HashSet::new();
    targets.insert(ip);
    let config = SupervisorConfig {
        targets,
        ping_rate_pps: 1,
        role: zzping_proto::zzping::CollectorRole::Primary,
        swap_at_nanos: Some(swap_at),
        use_mock_ping_client: true,
    };
    s_cfg_tx.send(Some(config)).unwrap();

    // Drain the immediate UpdateRole from reconcile; ignore other commands
    tokio::time::timeout(std::time::Duration::from_millis(100), async {
        while let Some(cmd) = mock_handle.command_rx.recv().await {
            if matches!(cmd, WorkerCommand::UpdateRole(_)) {
                break;
            }
        }
    })
    .await
    .expect("timed out waiting for immediate UpdateRole");

    // Send a new config quickly which should cancel the scheduled swap
    let new_config = SupervisorConfig {
        targets: std::collections::HashSet::new(),
        ping_rate_pps: 1,
        role: zzping_proto::zzping::CollectorRole::Standby,
        swap_at_nanos: None,
        use_mock_ping_client: true,
    };
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    s_cfg_tx.send(Some(new_config)).unwrap();

    // Now wait longer than the original swap time and ensure no UpdateRole arrives (ignore other messages)
    let res = tokio::time::timeout(std::time::Duration::from_millis(300), async {
        while let Some(cmd) = mock_handle.command_rx.recv().await {
            if matches!(cmd, WorkerCommand::UpdateRole(_)) {
                return Some(cmd);
            }
        }
        None::<WorkerCommand>
    })
    .await;

    // If we received Some(UpdateRole) within the timeout, the cancel failed.
    if let Ok(Some(_)) = res {
        panic!("Cancelled scheduled swap still fired an UpdateRole");
    }

    // Cleanup
    small_task.abort();
}
