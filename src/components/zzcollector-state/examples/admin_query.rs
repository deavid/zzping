//! Example: Admin querying the database for active collectors.

use std::time::Duration;
use tokio::time::sleep;
use zzcollector_state::builder::CStateBuilder;
use zzcollector_state::permissions::CStatePermission;
use zzcollector_state::role::CStateRole;
use zznet_session::session_manager::SessionManager;

#[actix::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let role = CStateRole::Admin;

    let builder = CStateBuilder::<
        zzcollector_state::network_messages::CStateMessage,
        CStatePermission,
        SessionManager<zzcollector_state::network_messages::CStateMessage, CStatePermission>,
    >::new(role);
    let _actor = builder.build();

    println!(
        "Admin actor started. Send QueryCollectors to database peers to receive CollectorList."
    );

    sleep(Duration::from_secs(10)).await;
    Ok(())
}
