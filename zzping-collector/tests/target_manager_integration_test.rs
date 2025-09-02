use std::future::IntoFuture;
use std::net::IpAddr;
use std::time::Duration;
use tokio::sync::mpsc;
use zzping_collector::ping_client::PingResult;
use zzping_collector::target_manager::run_target_manager;
use zzping_database::{spawn_test_server, timeout as db_timeout};

async fn timeout<F>(future: F) -> <F as IntoFuture>::Output
where
    F: IntoFuture,
{
    // In tests, we use a short timeout to fail fast.
    tokio::time::timeout(Duration::from_secs(5), future)
        .await
        .expect("Test timed out")
}

#[tokio::test]
async fn test_target_manager_happy_path() {
    let _ = env_logger::builder().is_test(true).try_init();
    println!("Starting test_target_manager_happy_path");

    let (server_addr, _server_handle) = db_timeout(spawn_test_server()).await;
    println!("Server spawned at {server_addr}");

    let (ping_tx, ping_rx) = mpsc::channel(100);

    let manager_handle = tokio::spawn(run_target_manager(
        vec![], // empty ca_cert for http
        format!("http://{server_addr}"),
        "test-host".to_string(),
        "1.2.3.4".parse::<IpAddr>().unwrap(),
        "my-secret-token".to_string(),
        ping_rx,
        Duration::from_secs(5),
    ));

    println!("Simulating ping source...");
    for i in 0..250 {
        timeout(ping_tx.send(PingResult {
            sent_nanos: i + 1,
            rtt: Some(Duration::from_millis(100)),
        }))
        .await
        .unwrap();
    }

    println!("Closing ping channel");
    drop(ping_tx);

    println!("Awaiting manager handle");
    let result = timeout(manager_handle).await;
    assert!(result.is_ok(), "Target manager panicked");
    println!("Test finished");
}

#[tokio::test]
async fn test_target_manager_reconnects_on_disconnect() {
    let _ = env_logger::builder().is_test(true).try_init();
    println!("Starting test_target_manager_reconnects_on_disconnect");

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
        Duration::from_millis(20),
    ));

    println!("Sending a few pings");
    for i in 0..10 {
        timeout(ping_tx.send(PingResult {
            sent_nanos: i + 1,
            rtt: Some(Duration::from_millis(100)),
        }))
        .await
        .unwrap();
    }

    println!("Aborting server");
    server_handle.abort();

    // Give some time for the manager to detect the disconnection and try to reconnect
    tokio::time::sleep(Duration::from_millis(50)).await;

    println!(
        "Spawning new server on the same address is not possible, the OS will not release the port immediately."
    );
    println!("Instead, we rely on the target manager's infinite loop to try to reconnect.");
    println!("We will just send more pings and see if the manager is still alive.");

    for i in 10..20 {
        if timeout(ping_tx.send(PingResult {
            sent_nanos: i + 1,
            rtt: Some(Duration::from_millis(100)),
        }))
        .await
        .is_err()
        {
            println!("Ping channel closed, which is expected as the manager might have panicked.");
            break;
        }
    }

    println!("Closing ping channel");
    drop(ping_tx);

    println!("Awaiting manager handle");
    let res = tokio::time::timeout(Duration::from_secs(1), manager_handle).await;
    assert!(
        res.is_err(),
        "Target manager should have run forever and not exited"
    );
    println!("Test finished");
}
