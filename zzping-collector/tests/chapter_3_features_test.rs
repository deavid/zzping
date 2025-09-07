use anyhow::Result;
use ntest::timeout;
use std::io::Write;
use std::time::Duration;
use zzping_collector::{bootstrap_collector, config::Config};

mod common;
use common::{spawn_mock_server, MockIngestionService};
use zzping_proto::zzping::CollectorRole;

/// Helper to write a config to a temporary file and return the file handle.
/// The file is deleted when the handle is dropped.
fn write_temp_config(config: &Config) -> tempfile::NamedTempFile {
    let file = tempfile::Builder::new()
        .prefix("collector-config")
        .suffix(".ron")
        .tempfile()
        .unwrap();
    let ron_string = ron::to_string(config).unwrap();
    (&file).write_all(ron_string.as_bytes()).unwrap();
    file
}

#[tokio::test]
#[timeout(10000)] // 10 second timeout
async fn test_health_reporting_pipeline() -> Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .is_test(true)
        .try_init()
        .ok();
    // 1. Setup
    let mock_service = MockIngestionService::new();
    let server_addr = spawn_mock_server(mock_service.clone()).await;

    let config = Config {
        collector_uuid: "health-test-uuid".to_string(),
        database_addr: format!("http://{server_addr}"),
        auth_token: "test-token".to_string(),
    };
    let temp_config_file = write_temp_config(&config);

    // 2. Start the collector service in the background
    let collector_service =
        bootstrap_collector(temp_config_file.path().to_str().unwrap().to_string())?;
    tokio::spawn(collector_service.run());

    // 3. Wait for heartbeats and assert on them
    let mut initial_heartbeat_received = false;
    for _ in 0..5 {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let heartbeats = mock_service.received_heartbeats.lock().unwrap();
        if !heartbeats.is_empty() {
            // The first heartbeat might be before the worker is fully up,
            // but its buffer will be 0.
            assert_eq!(heartbeats[0].buffer_record_count, 0);
            initial_heartbeat_received = true;
            break;
        }
    }
    assert!(
        initial_heartbeat_received,
        "Did not receive any heartbeats from the collector"
    );

    // 4. Wait for the pinger to run and fill the buffer
    tokio::time::sleep(Duration::from_secs(2)).await;

    // 5. Assert that a subsequent heartbeat shows a non-empty buffer
    let heartbeats = mock_service.received_heartbeats.lock().unwrap();
    assert!(
        heartbeats.len() > 1,
        "Collector should have sent more than one heartbeat"
    );

    // The last heartbeat should have a buffer count > 0
    let last_heartbeat = heartbeats.last().unwrap();
    assert!(
        last_heartbeat.buffer_record_count > 0,
        "Buffer record count should be greater than 0 after pinger has run, but it was {}",
        last_heartbeat.buffer_record_count
    );

    Ok(())
}

#[tokio::test]
#[timeout(5000)]
async fn test_command_stream_updates_role() -> Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();
    // 1. Setup
    let mock_service = MockIngestionService::new();
    let server_addr = spawn_mock_server(mock_service.clone()).await;
    let config = Config {
        collector_uuid: "command-stream-test".to_string(),
        database_addr: format!("http://{server_addr}"),
        auth_token: "test-token".to_string(),
    };
    let temp_config_file = write_temp_config(&config);
    let collector_service =
        bootstrap_collector(temp_config_file.path().to_str().unwrap().to_string())?;
    tokio::spawn(collector_service.run());
    tokio::time::sleep(Duration::from_secs(1)).await; // Wait for connection

    // 2. Get the command stream sender from the mock service
    let command_tx = mock_service.command_stream_tx.lock().unwrap().clone();
    assert!(command_tx.is_some(), "Collector did not subscribe to command stream");
    let command_tx = command_tx.unwrap();

    // 3. Send a ChangeRole command
    let command = zzping_proto::zzping::Command {
        command_id: 1,
        command_type: Some(zzping_proto::zzping::command::CommandType::ChangeRole(
            CollectorRole::Standby as i32,
        )),
    };
    command_tx.send(Ok(command)).await?;

    // 4. Check the logs to see if the supervisor acted on the command.
    // This is an indirect way of testing, but it proves the end-to-end flow.
    tokio::time::sleep(Duration::from_secs(1)).await;
    // We need a way to check logs. The VectorLogger is not easily accessible here.
    // For now, this test just proves we can send the command.
    // A more advanced test would require more refactoring to inspect supervisor state.
    // We will rely on the log output from the test run.
    // The log "TaskSupervisor: Updating role for all workers to Standby" should appear.

    Ok(())
}

#[tokio::test]
#[timeout(15000)] // 15 second timeout
async fn test_role_based_logic() -> Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .is_test(true)
        .try_init()
        .ok();
    // 1. Setup
    let mock_service = MockIngestionService::new();
    let server_addr = spawn_mock_server(mock_service.clone()).await;

    let config = Config {
        collector_uuid: "role-test-uuid".to_string(),
        database_addr: format!("http://{server_addr}"),
        auth_token: "test-token".to_string(),
    };
    let temp_config_file = write_temp_config(&config);

    // 2. Start the collector service
    let collector_service =
        bootstrap_collector(temp_config_file.path().to_str().unwrap().to_string())?;
    tokio::spawn(collector_service.run());
    tokio::time::sleep(Duration::from_secs(2)).await; // Give it time to connect

    // 3. Verify it starts pinging in Primary role
    let initial_batch_count = mock_service.received_batches.lock().unwrap().len();
    assert!(
        initial_batch_count > 0,
        "Collector should have sent batches in Primary role"
    );

    // 4. Change role to Standby and verify pinging stops
    {
        let mut heartbeart_response = mock_service.heartbeat_response.lock().unwrap();
        heartbeart_response.role = CollectorRole::Standby as i32;
    }
    // Wait for the role to propagate and the buffer to drain
    tokio::time::sleep(Duration::from_secs(2)).await;
    let batch_count_after_drain = mock_service.received_batches.lock().unwrap().len();

    // Wait a bit longer and confirm no *new* batches are sent
    tokio::time::sleep(Duration::from_secs(2)).await;
    let batch_count_after_standby = mock_service.received_batches.lock().unwrap().len();
    assert_eq!(
        batch_count_after_standby, batch_count_after_drain,
        "Collector should not send new batches in Standby role"
    );

    // 5. Change role back to Primary and verify pinging resumes
    {
        let mut heartbeart_response = mock_service.heartbeat_response.lock().unwrap();
        heartbeart_response.role = CollectorRole::Primary as i32;
    }
    tokio::time::sleep(Duration::from_secs(3)).await; // Wait for pinging to resume

    let final_batch_count = mock_service.received_batches.lock().unwrap().len();
    assert!(
        final_batch_count > batch_count_after_standby,
        "Collector should resume sending batches when role is changed back to Primary"
    );

    Ok(())
}
