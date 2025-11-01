//! Test harness for spawning and connecting demo services.

use crate::config::DemoAppConfig;
use crate::service::DemoAppService;
use zznet_hello::messages::GetRole;
use actix::Addr;
use zznet_api::mock::create_mock_pair;
use zznet_builder::traits::ZZNetService;
use zznet_hello::actor::HelloConfig;
use zznet_hello::connection_manager::HandleTransport;
use zznet_router::GetOfferedRooms;

/// Spawns a `DemoAppService` and returns the service and the address of ComponentA.
pub async fn spawn_demo_service(
    config: DemoAppConfig,
) -> (DemoAppService, Addr<crate::component_a::ComponentAActor>) {
    let service = DemoAppService::new(config).unwrap();
    let comp_a_addr = service.component_a.clone();
    (service, comp_a_addr)
}

/// Connects two `DemoAppService` instances using a mock transport.
pub async fn connect_services(service_a: &DemoAppService, service_b: &DemoAppService) {
    let (transport_a, transport_b) = create_mock_pair("test");

    let offered_rooms_a = service_a
        .router
        .send(GetOfferedRooms)
        .await
        .unwrap()
        .iter()
        .map(|r| r.as_str().to_string())
        .collect();

    let offered_rooms_b = service_b
        .router
        .send(GetOfferedRooms)
        .await
        .unwrap()
        .iter()
        .map(|r| r.as_str().to_string())
        .collect();

    let config_a = HelloConfig {
        our_role: service_a.connection_manager.send(GetRole).await.unwrap(),
        offered_rooms: offered_rooms_a,
        handshake_timeout: std::time::Duration::from_secs(1),
        hostname: "service_a".to_string(),
    };

    let config_b = HelloConfig {
        our_role: service_b.connection_manager.send(GetRole).await.unwrap(),
        offered_rooms: offered_rooms_b,
        handshake_timeout: std::time::Duration::from_secs(1),
        hostname: "service_b".to_string(),
    };

    service_a
        .connection_manager
        .send(HandleTransport {
            transport: Box::new(transport_a),
            config: config_a,
        })
        .await
        .unwrap()
        .unwrap();

    service_b
        .connection_manager
        .send(HandleTransport {
            transport: Box::new(transport_b),
            config: config_b,
        })
        .await
        .unwrap()
        .unwrap();
}
