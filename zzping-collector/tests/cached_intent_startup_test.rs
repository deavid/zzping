// Integration tests for last-known intent cache startup behavior.

use anyhow::Result;
use ntest::timeout;
use std::time::Duration;
use tempfile::TempDir;

// Bring the collector bootstrap function used by some tests
use zzping_collector::run_with_config_path;

// Shared test utilities (mock server, logger)
mod common;
use common::{MockIngestionService, VectorLogger};

/// Create a temporary test workspace with optional last_intent content and a collector config
fn setup_test_environment(
    last_intent_content: Option<&str>,
    db_addr: &str,
) -> Result<(TempDir, String)> {
    let temp_dir = tempfile::tempdir()?;
    let dir_path = temp_dir.path();

    let collector_config_content = format!(
        r#"(
    collector_uuid: "cached-intent-test-uuid",
    database_addr: "{}",
    auth_token: "test-token",
    use_mock_ping_client: true,
)
"#,
        db_addr
    );

    let collector_config_path = dir_path.join("collector.ron");
    std::fs::write(&collector_config_path, collector_config_content)?;

    if let Some(content) = last_intent_content {
        let last_intent_path = dir_path.join("last_intent.ron");
        std::fs::write(last_intent_path, content)?;
    }

    Ok((
        temp_dir,
        collector_config_path.to_str().unwrap().to_string(),
    ))
}

#[tokio::test]
#[timeout(5000)]
async fn startup_with_cache_and_unavailable_db() -> Result<()> {
    // Collector has a cached intent but DB is unreachable -> supervisor should defer worker creation

    let log_messages = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let _ = VectorLogger::init(log_messages.clone());

    let (temp_dir, config_path) = setup_test_environment(
        Some(
            r#"(
    targets: ["8.8.8.8"],
    ping_rate_pps: 50,
)
"#,
        ),
        "http://127.0.0.1:9999",
    )?;

    std::env::set_current_dir(temp_dir.path())?;

    let svc = tokio::spawn(async move { run_with_config_path(config_path).await });

    // Allow startup to proceed
    tokio::time::sleep(Duration::from_millis(200)).await;

    assert!(!svc.is_finished(), "Collector should still be running");

    let guard = log_messages.lock().unwrap();
    assert!(
        guard
            .iter()
            .any(|s| s.contains("Deferring worker creation")),
        "Expected supervisor to defer worker creation when DB unavailable",
    );

    Ok(())
}

#[tokio::test]
#[timeout(8000)]
async fn startup_with_cache_and_successful_connection() -> Result<()> {
    // Deterministic: use mock ingestion service and heartbeat override to cause the collector to
    // receive a heartbeat, which should cause the supervisor to reconcile and spawn workers.

    let log_messages = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let _ = VectorLogger::init(log_messages.clone());

    let mock_service = MockIngestionService::new();
    let addr = common::spawn_mock_server(mock_service.clone()).await;

    // Heartbeat we will inject when ready
    let heartbeat = zzping_proto::zzping::HeartbeatResponse {
        targets: vec!["8.8.8.8".to_string()],
        ping_rate_pps: 100,
        role: zzping_proto::zzping::CollectorRole::Primary as i32,
        swap_at_nanos: 0,
        last_fsynced_received_nanos: 0,
    };

    let hb_override_tx = mock_service.install_heartbeat_override_channel().await;

    let (temp_dir, config_path) = setup_test_environment(
        Some(
            r#"(
    targets: ["1.1.1.1"],
    ping_rate_pps: 50,
)
"#,
        ),
        &format!("http://{}", addr),
    )?;

    std::env::set_current_dir(temp_dir.path())?;

    // Use test bootstrap helper to receive worker count updates
    let (worker_tx, mut worker_rx) = tokio::sync::mpsc::channel::<usize>(8);
    let (shutdown_sender_tx, mut shutdown_sender_rx) = tokio::sync::mpsc::channel(1);
    let service = zzping_collector::bootstrap_collector_for_test_with_worker_tx(
        config_path,
        shutdown_sender_tx,
        Some(worker_tx),
    )?;

    let svc_handle = tokio::spawn(async move { service.run().await });

    // Inject heartbeat immediately to ensure the mock server will respond
    // deterministically when the collector calls heartbeat.
    hb_override_tx
        .send(heartbeat)
        .await
        .expect("failed to send heartbeat override");

    // Wait for worker count to become > 0 deterministically
    let saw = tokio::time::timeout(Duration::from_millis(3000), async {
        loop {
            match worker_rx.recv().await {
                Some(cnt) => {
                    if cnt > 0 {
                        break true;
                    }
                }
                None => break false,
            }
        }
    })
    .await
    .unwrap_or(false);

    assert!(
        saw,
        "Expected supervisor to spawn workers after receiving heartbeat"
    );

    // Shutdown service gracefully
    if let Some(shutdown_tx) =
        tokio::time::timeout(Duration::from_millis(500), shutdown_sender_rx.recv())
            .await
            .ok()
            .flatten()
    {
        let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
        shutdown_tx
            .send(zzping_collector::task_supervisor::SupervisorShutdown { ack_sender: ack_tx })
            .await
            .ok();
        let _ = tokio::time::timeout(Duration::from_millis(500), ack_rx).await;
    }

    let _ = svc_handle.await;

    Ok(())
}

#[tokio::test]
#[timeout(5000)]
async fn startup_without_cache_and_unavailable_db() -> Result<()> {
    // When no cache exists and DB is unreachable, the collector should not spawn workers

    let log_messages = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let _ = VectorLogger::init(log_messages.clone());

    let (temp_dir, config_path) = setup_test_environment(None, "http://127.0.0.1:9999")?;
    std::env::set_current_dir(temp_dir.path())?;

    let svc = tokio::spawn(async move { run_with_config_path(config_path).await });
    tokio::time::sleep(Duration::from_millis(200)).await;

    assert!(!svc.is_finished(), "Collector should still be running");

    let guard = log_messages.lock().unwrap();
    assert!(
        !guard
            .iter()
            .any(|s| s.contains("TaskSupervisor: Adding worker")),
        "Should not have created workers without cache"
    );

    Ok(())
}
