//! Example: Collector role sending periodic heartbeats to the network.

use std::time::Duration;
use tokio::time::sleep;
use zzcollector_state::builder::CStateBuilder;
use zzcollector_state::permissions::CStatePermission;
use zzcollector_state::role::CStateRole;
use zznet_session::session_manager::SessionManager;

#[actix::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // NOTE: This example assumes a working SessionManager configured for your
    // environment. For real usage, supply a configured session manager instance.

    // Example: start as a Collector, heartbeat every 5 seconds
    let role = CStateRole::Collector {
        collector_id: "example-collector".to_string(),
        heartbeat_interval_secs: 5,
    };

    // For demonstrations, we don't have a real SessionManager here.
    let builder = CStateBuilder::<
        zzcollector_state::network_messages::CStateMessage,
        CStatePermission,
        SessionManager<zzcollector_state::network_messages::CStateMessage, CStatePermission>,
    >::new(role);
    let _actor = builder.build();

    println!("Collector actor started (example). Run a real SessionManager to observe heartbeats.");

    // Keep process alive to allow heartbeats to fire
    sleep(Duration::from_secs(20)).await;

    Ok(())
}
