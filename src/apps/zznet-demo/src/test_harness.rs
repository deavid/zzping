//! Test harness for spawning and connecting demo services.

use crate::config::DemoAppConfig;
use crate::service::DemoAppService;
use actix::Addr;
use zznet_api::create_mock_pair;
use zznet_hello::HandleTransport;
use zznet_hello::HelloConfig;

/// Spawns a `DemoAppService` and returns the service and the address of ComponentA.
pub async fn spawn_demo_service(
    config: DemoAppConfig,
) -> (DemoAppService, Addr<crate::component_a::ComponentAActor>) {
    // Create the service directly (no builder needed)
    let service = DemoAppService::new(config.clone()).unwrap();
    let comp_a_addr = service.component_a.clone();

    (service, comp_a_addr)
}

/// Wires two demo services together using a mock transport for integration testing.
pub async fn connect_services(
    service_a: &DemoAppService,
    config_a: &DemoAppConfig,
    service_b: &DemoAppService,
    config_b: &DemoAppConfig,
) {
    let (transport_a, transport_b) = create_mock_pair("test");

    let hello_config_a = HelloConfig {
        our_role: config_a.our_role.clone(),
        offered_rooms: config_a.offered_rooms.clone(),
        hostname: format!("service_a_host_{}", config_a.our_role), // Example hostname
        handshake_timeout: std::time::Duration::from_secs(1),
    };

    let hello_config_b = HelloConfig {
        our_role: config_b.our_role.clone(),
        offered_rooms: config_b.offered_rooms.clone(),
        hostname: format!("service_b_host_{}", config_b.our_role), // Example hostname
        handshake_timeout: std::time::Duration::from_secs(1),
    };

    service_a
        .connection_manager
        .send(HandleTransport {
            transport: Box::new(transport_a),
            config: hello_config_a,
        })
        .await
        .unwrap()
        .unwrap();

    service_b
        .connection_manager
        .send(HandleTransport {
            transport: Box::new(transport_b),
            config: hello_config_b,
        })
        .await
        .unwrap()
        .unwrap();
}
