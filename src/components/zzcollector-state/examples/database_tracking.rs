//! Example: Database role tracking collectors.

use std::time::Duration;
use tokio::time::sleep;
use zzcollector_state::builder::CStateBuilder;
use zzcollector_state::permissions::CStatePermission;
use zzcollector_state::role::CStateRole;
use zznet_session::session_manager::SessionManager;

#[actix::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let role = CStateRole::Database {
        stale_timeout_secs: 15,
        max_collectors: None,
    };

    let builder = CStateBuilder::<
        zzcollector_state::network_messages::CStateMessage,
        CStatePermission,
        SessionManager<zzcollector_state::network_messages::CStateMessage, CStatePermission>,
    >::new(role);
    let _actor = builder.build();

    println!("Database actor started (example). Connect collectors to see tracking in action.");

    // Keep running to allow collector heartbeats to arrive
    sleep(Duration::from_secs(60)).await;
    Ok(())
}
