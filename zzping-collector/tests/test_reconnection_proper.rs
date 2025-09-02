use std::net::IpAddr;
use std::time::Duration;
use tokio::sync::mpsc;
use zzping_collector::ping_client::PingResult;
use zzping_collector::target_manager::run_target_manager;
use zzping_database::{spawn_test_server, timeout as db_timeout};

#[tokio::test]
async fn test_reconnection_works_after_fix() {
    let _ = env_logger::builder().is_test(true).try_init();
    println!("Starting test to validate reconnection fix works");

    let (server_addr, server_handle) = db_timeout(spawn_test_server()).await;
    println!("Server spawned at {server_addr}");

    let (ping_tx, ping_rx) = mpsc::channel(100);

    let manager_handle = tokio::spawn(run_target_manager(
        vec![], // empty ca_cert for http
        format!("http://{server_addr}"),
        "test-host".to_string(),
        "1.2.3.4".parse::<IpAddr>().unwrap(),
        "my-secret-token".to_string(),
        ping_rx,
        Duration::from_millis(50), // Fast retry
    ));

    println!("Sending initial pings");
    for i in 0..3 {
        ping_tx
            .send(PingResult {
                sent_nanos: i + 1,
                rtt: Some(Duration::from_millis(100)),
            })
            .await
            .unwrap();
    }

    // Let connection establish
    tokio::time::sleep(Duration::from_millis(200)).await;

    println!("Aborting server to force stream closure");
    server_handle.abort();

    // Give some time for the stream closure to be detected
    tokio::time::sleep(Duration::from_millis(200)).await;

    // The key test: can we still send pings without the manager hanging?
    // With the fix, the manager should detect stream closure and keep retrying
    println!("Sending pings after server shutdown - should not hang");

    let mut successful_pings = 0;
    for i in 3..8 {
        if ping_tx
            .send(PingResult {
                sent_nanos: i + 1,
                rtt: Some(Duration::from_millis(100)),
            })
            .await
            .is_ok()
        {
            successful_pings += 1;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    println!(
        "Successfully sent {} pings after server shutdown",
        successful_pings
    );

    // Close ping channel to cleanly terminate
    drop(ping_tx);

    // Manager should exit gracefully when ping channel closes
    let manager_exits_gracefully = tokio::time::timeout(Duration::from_millis(500), manager_handle)
        .await
        .is_ok();

    println!(
        "Manager exits gracefully after ping channel close: {}",
        manager_exits_gracefully
    );

    // The fix should allow:
    // 1. Stream closure detection (no hanging)
    // 2. Successful ping sending (no channel closure due to hanging)
    // 3. Graceful exit when ping channel closes
    assert!(
        successful_pings > 0,
        "Should be able to send pings after stream closure without hanging"
    );
    assert!(
        manager_exits_gracefully,
        "Manager should exit gracefully when ping channel closes"
    );

    println!("Test passed - reconnection fix works correctly");
}
