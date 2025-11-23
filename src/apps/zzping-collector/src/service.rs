//! Collector service and component orchestration for the zzping collector application.
//!
//! This module defines `CollectorService`, the top-level application service that
//! configures, starts, and coordinates all collector components.

use crate::config::CollectorTlsConfig;
use actix::Addr;
use anyhow::Result;
use zzintent_config::actor::IntentConfigActor;
use zzmem_db::actor::MemDBActor;
use zznet_router::RouterActor;
use zzpinger::scheduler::PingerSchedulerActor;

/// Started components (running actors)
pub struct StartedComponents {
    /// Address of the running IntentConfig actor.
    pub intent_config: Addr<IntentConfigActor>,
    /// Address of the running Pinger scheduler actor.
    pub pinger: Addr<PingerSchedulerActor>,
    /// Address of the running MemDB actor.
    pub memdb_addr: Addr<MemDBActor>,
    /// RouterActor for data-plane message routing.
    pub router_actor: Addr<RouterActor>,
}

/// Convert collector TLS config to transport layer TLS config
pub fn convert_tls_config(
    tls: &CollectorTlsConfig,
) -> Result<zznet_transport_tcp::config::TlsConfig> {
    Ok(zznet_transport_tcp::tls_utils::to_transport_tls_config(
        &tls.client_cert_path,
        &tls.client_key_path,
        Some(&tls.ca_cert_path),
        "zzping-mesh".into(),
    ))
}
