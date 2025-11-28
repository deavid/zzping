//! Test harness for spawning and connecting demo services.

use crate::config::DemoAppConfig;
use crate::service::DemoAppService;
use actix::Addr;
use zznet_api::{AcceptTransport, create_mock_pair};

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
pub async fn connect_services(service_a: &DemoAppService, service_b: &DemoAppService) {
    let (transport_a, transport_b) = create_mock_pair("test");

    let conn_a = transport_a.into_established();
    service_a
        .connection_manager
        .send(AcceptTransport {
            tx: conn_a.tx,
            rx: conn_a.rx,
            peer_addr: conn_a.peer_addr,
            peer_identity: conn_a.peer_identity,
        })
        .await
        .unwrap();

    let conn_b = transport_b.into_established();
    service_b
        .connection_manager
        .send(AcceptTransport {
            tx: conn_b.tx,
            rx: conn_b.rx,
            peer_addr: conn_b.peer_addr,
            peer_identity: conn_b.peer_identity,
        })
        .await
        .unwrap();
}
