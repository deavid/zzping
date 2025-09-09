use ntest::timeout;
use std::{collections::HashSet, net::IpAddr, str::FromStr};
use zzping_collector::target_worker::WorkerCommand;
use zzping_collector::task_supervisor::{SupervisorConfig, TaskSupervisor};
use zzping_proto::zzping::CollectorRole;

mod common;
use common::mock_worker_factory;

#[tokio::test]
#[timeout(1000)]
async fn test_supervisor_sends_update_role_on_config_change() {
    // 1. Setup
    let mut supervisor = TaskSupervisor::new("test-uuid".to_string(), 1);

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
#[timeout(1000)]
async fn test_supervisor_sends_shutdown_to_removed_workers() {
    // 1. Setup
    let mut supervisor = TaskSupervisor::new("test-uuid".to_string(), 1);

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
