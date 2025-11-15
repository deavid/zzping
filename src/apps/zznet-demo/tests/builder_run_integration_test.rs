//! Run demo services via `zznet-builder` in background and verify connectivity.
use zznet_demo::{config::DemoAppConfig, test_harness::spawn_demo_service_with_builder};

#[tokio::test]
async fn test_builder_run_services_connect_and_message() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async move {
            let cfg_a = DemoAppConfig::new("app1", None);
            let cfg_b = DemoAppConfig::new("app2", Some("app1".to_string()));

            let (handle_a, _service_a, comp_a_addr, stop_a) = spawn_demo_service_with_builder(cfg_a).await;
            let (handle_b, service_b, _comp_b_addr, stop_b) = spawn_demo_service_with_builder(cfg_b).await;

            // Connect the services using the existing helper
            zznet_demo::test_harness::connect_services(&service_b, &_service_a).await;

            // Give them time to handshake
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;

            // Send a ping from A
            comp_a_addr.do_send(zznet_demo::messages::SendPing { data: "hello".into() });

            tokio::time::sleep(std::time::Duration::from_millis(200)).await;

            // Check B received it; service_b has component addresses
            let counter = service_b.component_a.send(zznet_demo::messages::GetCounter).await.unwrap();
            assert_eq!(counter, 1);

            // Request shutdown for both services
            let _ = stop_a.send(());
            let _ = stop_b.send(());

            // Wait for background handles to complete
            let _ = handle_a.await;
            let _ = handle_b.await;
        })
        .await;
}
