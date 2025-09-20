use ntest::timeout;
use zznet::connection::{ClientConfig, ServerConfig};
use zznet_api::Role;
use zznet_lib::{ZzNetApi, ZzNetBuilder, ZzNetConfig};

#[tokio::test]
#[timeout(200)]
async fn zznet_lib_actor_lifecycle_and_shutdown() {
    let _ = env_logger::builder().is_test(true).try_init();
    log::info!("Test starting: zznet_lib_actor_lifecycle_and_shutdown");

    log::info!("Step 1: Setting up server configuration");
    let server_config = ServerConfig {
        socketaddr: vec!["127.0.0.1:0".parse().unwrap()],
        tls: None,
        role: Role::Collector,
    };
    let config = ZzNetConfig::Server(server_config);
    let builder = ZzNetBuilder::new(config);

    log::info!("Step 2: Starting the component");
    let handle = builder
        .start()
        .await
        .expect("Component should start successfully");
    log::info!("Component started, handle acquired.");

    log::info!("Step 3: Shutting down the component");
    handle
        .shutdown()
        .await
        .expect("Component should shut down cleanly");
    log::info!("Component shutdown complete.");
    log::info!("Test finished: zznet_lib_actor_lifecycle_and_shutdown");
}

#[tokio::test]
#[timeout(200)]
async fn zznet_lib_client_api_fails_when_not_connected() {
    let _ = env_logger::builder().is_test(true).try_init();
    log::info!("Test starting: zznet_lib_client_api_fails_when_not_connected");

    log::info!("Step 1: Setting up client configuration");
    let client_config = ClientConfig {
        socketaddr: vec!["127.0.0.1:1".parse().unwrap()],
        tls: None,
        role: Role::Collector,
        reconnect_delay: std::time::Duration::from_secs(10),
    };
    let config = ZzNetConfig::Client(client_config);
    let builder = ZzNetBuilder::new(config);

    log::info!("Step 2: Starting the component");
    let handle = builder
        .start()
        .await
        .expect("Component should start even if it cannot connect");
    log::info!("Component started, handle acquired.");

    log::info!("Step 3: Calling request_channel, expecting a failure");
    // The API is now on the handle itself.
    let result = handle.request_channel("test".to_string()).await;
    assert!(
        result.is_err(),
        "request_channel should fail if not connected"
    );
    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("Not connected"),
        "Error message should indicate not connected"
    );
    log::info!("Received expected error: {err}");

    log::info!("Step 4: Shutting down the component");
    handle
        .shutdown()
        .await
        .expect("Component should shut down cleanly");
    log::info!("Component shutdown complete.");
    log::info!("Test finished: zznet_lib_client_api_fails_when_not_connected");
}
