//! Builder-based integration tests for the zznet-demo application.
use zznet_demo::{
    config::DemoAppConfig,
    messages::{GetCounter, PublishToA, SendPing, SendPingFromB},
    test_harness::{connect_services, spawn_demo_service},
};

#[tokio::test]
async fn test_builder_unauthorized_connection_is_rejected() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async move {
            let cfg_a = DemoAppConfig::new("app1", None);
            let (service_a, comp_a_addr) = spawn_demo_service(cfg_a.clone()).await;
            let cfg_b = DemoAppConfig::new("app3", Some("app1".to_string()));
            let (service_b, comp_b_addr) = spawn_demo_service(cfg_b.clone()).await;

            connect_services(&service_a, &cfg_a, &service_b, &cfg_b).await;
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            comp_a_addr.do_send(SendPing {
                data: "test".to_string(),
            });

            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            let counter = comp_b_addr.send(GetCounter).await.unwrap();
            assert_eq!(counter, 0);
        })
        .await;
}

#[tokio::test]
async fn test_builder_multiple_components() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async move {
            let cfg_a = DemoAppConfig::new("app1", None);
            let (service_a, comp_a_addr) = spawn_demo_service(cfg_a.clone()).await;
            let cfg_b = DemoAppConfig::new("app2", Some("app1".to_string()));
            let (service_b, _comp_b_addr) = spawn_demo_service(cfg_b.clone()).await;

            connect_services(&service_a, &cfg_a, &service_b, &cfg_b).await;
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;

            comp_a_addr.do_send(PublishToA {
                data: "test".to_string(),
            });

            tokio::time::sleep(std::time::Duration::from_millis(300)).await;

            let counter = service_b
                .component_b
                .as_ref()
                .unwrap()
                .send(GetCounter)
                .await
                .unwrap();
            assert_eq!(counter, 1);
        })
        .await;
}

#[tokio::test]
async fn test_builder_component_b_sends_message_via_component_a() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async move {
            let cfg_a = DemoAppConfig::new("app1", None);
            let (service_a, comp_a_addr) = spawn_demo_service(cfg_a.clone()).await;
            let cfg_b = DemoAppConfig::new("app2", Some("app1".to_string()));
            let (service_b, _comp_b_addr) = spawn_demo_service(cfg_b.clone()).await;

            connect_services(&service_a, &cfg_a, &service_b, &cfg_b).await;
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            service_b
                .component_b
                .as_ref()
                .unwrap()
                .do_send(SendPingFromB {
                    data: "test".to_string(),
                });

            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            let counter = comp_a_addr.send(GetCounter).await.unwrap();
            assert_eq!(counter, 1);
        })
        .await;
}

#[tokio::test]
async fn test_builder_basic_connection() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async move {
            let cfg_a = DemoAppConfig::new("app1", None);
            let (service_a, comp_a_addr) = spawn_demo_service(cfg_a.clone()).await;
            let cfg_b = DemoAppConfig::new("app2", Some("app1".to_string()));
            let (service_b, _comp_b_addr) = spawn_demo_service(cfg_b.clone()).await;

            connect_services(&service_a, &cfg_a, &service_b, &cfg_b).await;
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            comp_a_addr.do_send(SendPing {
                data: "test".to_string(),
            });

            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            let counter = comp_a_addr.send(GetCounter).await.unwrap();
            assert_eq!(counter, 1);
        })
        .await;
}
