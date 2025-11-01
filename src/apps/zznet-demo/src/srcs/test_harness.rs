//! Test harness for spawning and connecting `DemoAppService` instances.
use crate::{config::DemoAppConfig, service::DemoAppService};
use actix::prelude::*;
use anyhow::Result;
use tokio::sync::oneshot;
use zznet_api::mock::create_mock_pair;
use zznet_hello::{actor::HelloConfig, connection_manager::HandleTransport};

/// A handle to a running `DemoAppService` instance.
pub struct ServiceHandle {
    /// The service instance.
    pub service: DemoAppService,
    /// A channel to send a shutdown signal to the service.
    pub shutdown_tx: oneshot::Sender<()>,
}

/// Spawns a `DemoAppService` instance in a new tokio task.
pub async fn spawn_demo_service(config: DemoAppConfig) -> Result<(Addr<System>, ServiceHandle)> {
    let (tx, rx) = oneshot::channel();
    let (shutdown_tx, shutdown_rx) = oneshot::channel();

    let handle = std::thread::spawn(move || {
        let system = System::new();
        system.block_on(async move {
            let service = DemoAppService::new(config).unwrap();
            tx.send(service.component_a.clone()).unwrap();
            tokio::select! {
                _ = service.run() => {},
                _ = shutdown_rx => {},
            }
        });
    });

    let component_a_addr = rx.await?;
    let service = ServiceHandle {
        service: DemoAppService {
            router: Addr::default(),
            connection_manager: Addr::default(),
            component_a: component_a_addr,
            component_b: None,
        },
        shutdown_tx,
    };
    Ok((System::current(), service))
}

/// Connects two `DemoAppService` instances using mock transport.
pub async fn connect_services(service_a: &ServiceHandle, service_b: &ServiceHandle) -> Result<()> {
    let (conn_a, conn_b) = create_mock_pair("test");

    let hello_config_a = HelloConfig {
        our_role: "app1".to_string(),
        offered_rooms: vec!["room-a".to_string()],
        ..Default::default()
    };

    let hello_config_b = HelloConfig {
        our_role: "app2".to_string(),
        offered_rooms: vec!["room-a".to_string()],
        ..Default::default()
    };

    service_a
        .service
        .connection_manager
        .do_send(HandleTransport {
            transport: Box::new(conn_a),
            config: hello_config_a,
        });

    service_b
        .service
        .connection_manager
        .do_send(HandleTransport {
            transport: Box::new(conn_b),
            config: hello_config_b,
        });

    Ok(())
}
