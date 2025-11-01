//! Builder-based integration tests for the zznet-demo application.
use anyhow::Result;
use zznet_demo::{
    config::DemoAppConfig,
    messages::{GetCounter, PublishToA, SendPing, SendPingFromB},
    test_harness::{connect_services, spawn_demo_service},
};
use zznet_builder::traits::ZZNetConfig;

#[tokio::test]
async fn test_builder_ping_pong_between_component_a() -> Result<()> {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async move {
            let (service_a, comp_a_addr) =
                spawn_demo_service(DemoAppConfig::new("app1", None)).await;
            let (service_b, comp_b_addr) =
                spawn_demo_service(DemoAppConfig::new("app2", Some("app1".to_string()))).await;

            connect_services(&service_a, &service_b).await;
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            comp_a_addr.do_send(SendPing {
                data: "test".to_string(),
            });

            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            let counter = comp_b_addr.send(GetCounter).await?;
            assert_eq!(counter, 1);

            Ok(())
        })
        .await
}

#[test]
fn test_builder_config_validation_rejects_invalid() {
    let config = DemoAppConfig {
        our_role: "".to_string(), // Invalid: empty role
        peer_addr: None,
        allowed_roles: vec!["database".to_string()],
        offered_rooms: vec!["room-a".to_string()],
        include_component_b: false,
    };
    assert!(config.validate().is_err());
}

#[test]
fn test_builder_config_validation_accepts_valid() {
    let config = DemoAppConfig {
        our_role: "collector".to_string(),
        peer_addr: None,
        allowed_roles: vec!["database".to_string()],
        offered_rooms: vec!["room-a".to_string()],
        include_component_b: false,
    };
    assert!(config.validate().is_ok());
}

#[test]
fn test_builder_config_serialization_roundtrip() -> Result<()> {
    let config = DemoAppConfig {
        our_role: "collector".to_string(),
        peer_addr: Some("database".to_string()),
        allowed_roles: vec!["database".to_string()],
        offered_rooms: vec!["room-a".to_string()],
        include_component_b: true,
    };

    let serialized = ron::to_string(&config)?;
    let deserialized: DemoAppConfig = ron::from_str(&serialized)?;

    assert_eq!(config.our_role, deserialized.our_role);
    assert_eq!(config.peer_addr, deserialized.peer_addr);
    assert_eq!(config.allowed_roles, deserialized.allowed_roles);
    assert_eq!(config.offered_rooms, deserialized.offered_rooms);
    assert_eq!(config.include_component_b, deserialized.include_component_b);

    Ok(())
}

#[tokio::test]
async fn test_builder_component_a_publishes_to_component_b() -> Result<()> {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async move {
            let (service_a, comp_a_addr) =
                spawn_demo_service(DemoAppConfig::new("app1", None)).await;
            let (service_b, _comp_b_addr) =
                spawn_demo_service(DemoAppConfig::new("app2", Some("app1".to_string()))).await;

            connect_services(&service_a, &service_b).await;
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            comp_a_addr.do_send(PublishToA {
                data: "test".to_string(),
            });

            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            let counter = service_b
                .component_b
                .as_ref()
                .unwrap()
                .send(GetCounter)
                .await?;
            assert_eq!(counter, 1);

            Ok(())
        })
        .await
}

#[tokio::test]
async fn test_builder_component_b_sends_message_via_component_a() -> Result<()> {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async move {
            let (service_a, comp_a_addr) =
                spawn_demo_service(DemoAppConfig::new("app1", None)).await;
            let (service_b, _comp_b_addr) =
                spawn_demo_service(DemoAppConfig::new("app2", Some("app1".to_string()))).await;

            connect_services(&service_a, &service_b).await;
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            service_b
                .component_b
                .as_ref()
                .unwrap()
                .do_send(SendPingFromB {
                    data: "test".to_string(),
                });

            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            let counter = comp_a_addr.send(GetCounter).await?;
            assert_eq!(counter, 1);

            Ok(())
        })
        .await
}

#[tokio::test]
async fn test_builder_unauthorized_connection_is_rejected() -> Result<()> {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async move {
            let (service_a, comp_a_addr) =
                spawn_demo_service(DemoAppConfig::new("app1", None)).await;
            let (service_b, comp_b_addr) =
                spawn_demo_service(DemoAppConfig::new("app3", Some("app1".to_string()))).await;

            connect_services(&service_a, &service_b).await;
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            comp_a_addr.do_send(SendPing {
                data: "test".to_string(),
            });

            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            let counter = comp_b_addr.send(GetCounter).await?;
            assert_eq!(counter, 0);

            Ok(())
        })
        .await
}
