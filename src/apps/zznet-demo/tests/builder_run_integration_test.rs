//! Run demo services via `zznet-builder` harness in background and verify connectivity.
use zznet_demo::{config::DemoAppConfig, test_harness::spawn_demo_service_with_builder};

#[tokio::test(flavor = "current_thread")]
async fn test_harness_run_services_connect_and_message() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async move {
            let cfg_a = DemoAppConfig::new("app1", None);
            let cfg_b = DemoAppConfig::new("app2", Some("app1".to_string()));
            let (service_a, comp_a_addr) = spawn_demo_service_with_builder(cfg_a.clone()).await;
            let (service_b, _comp_b_addr) = spawn_demo_service_with_builder(cfg_b.clone()).await;
            // Connect the services using the existing helper
            zznet_demo::test_harness::connect_services(&service_a, &cfg_a, &service_b, &cfg_b)
                .await;

            // Give them time to handshake
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;

            // Send a ping from A
            comp_a_addr.do_send(zznet_demo::messages::SendPing {
                data: "hello".into(),
            });

            tokio::time::sleep(std::time::Duration::from_millis(1)).await;

            // Check B received it; service_b has component addresses
            let counter = service_b
                .component_a
                .send(zznet_demo::messages::GetCounter)
                .await
                .unwrap();
            assert_eq!(counter, 1);
        })
        .await;
}
