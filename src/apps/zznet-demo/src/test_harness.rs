//! Test harness for spawning and connecting demo services.

use crate::config::DemoAppConfig;
use crate::service::DemoAppService;
use actix::Addr;
use zznet_api::mock::create_mock_pair;
use zznet_builder::harness::AppHarness;
use zznet_hello::actor::HelloConfig;
use zznet_hello::connection_manager::HandleTransport;

/// Spawns a `DemoAppService` and returns the service and the address of ComponentA.
pub async fn spawn_demo_service(
    config: DemoAppConfig,
) -> (DemoAppService, Addr<crate::component_a::ComponentAActor>) {
    // Create the service directly (no builder needed)
    let service = DemoAppService::new(config.clone()).unwrap();
    let comp_a_addr = service.component_a.clone();
    (service, comp_a_addr)
}

/// Spawn the demo service using the `zznet-builder` API. This will run the
/// full app lifecycle in a background thread using the builder's `run_service_with_config_and_stop`.
/// Returns the spawned demo service handles and a stop channel sender that can be used to request shutdown.
pub async fn spawn_demo_service_with_builder(
    config: DemoAppConfig,
) -> (
    tokio::task::JoinHandle<()>,
    DemoAppService,
    Addr<crate::component_a::ComponentAActor>,
    tokio::sync::oneshot::Sender<()>,
) {
    // Create the service directly
    let service = DemoAppService::new(config.clone()).unwrap();
    let comp_a_addr = service.component_a.clone();

    // Create a programmatic stop channel
    let (stop_tx, _stop_rx) = tokio::sync::oneshot::channel::<()>();

    // Spawn a task that runs the service with harness
    let cfg_clone = config.clone();
    let handle = tokio::task::spawn(async move {
        let harness = AppHarness::new().log_level("info");
        harness.init_logging();

        let app = DemoAppService::new(cfg_clone).unwrap();
        let _ = harness.run(app);
    });

    (handle, service, comp_a_addr, stop_tx)
}

/// Connects two `DemoAppService` instances using a mock transport.
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
