//! Configuration for the demo application.

use serde::{Deserialize, Serialize};
use zznet_builder::traits::ZZNetConfig;

/// Configuration for the demo application.
#[derive(Debug, Deserialize, Serialize)]
pub struct DemoAppConfig {
    /// The role of this application instance.
    pub our_role: String,
    /// The address of the peer to connect to.
    pub peer_addr: Option<String>,
    /// The rooms offered by this application instance.
    pub offered_rooms: Vec<String>,
    /// The roles allowed to connect to this application instance.
    pub allowed_roles: Vec<String>,
    /// Whether to include ComponentB in the application.
    pub include_component_b: bool,
}

impl DemoAppConfig {
    pub fn new(our_role: &str, peer_addr: Option<String>) -> Self {
        Self {
            our_role: our_role.to_string(),
            peer_addr: peer_addr.clone(),
            offered_rooms: vec!["room-a".to_string()],
            allowed_roles: vec!["app1".to_string(), "app2".to_string()],
            include_component_b: peer_addr.is_some(),
        }
    }
}

impl ZZNetConfig for DemoAppConfig {
    fn validate(&self) -> anyhow::Result<()> {
        if self.our_role.is_empty() {
            anyhow::bail!("our_role cannot be empty");
        }
        Ok(())
    }

    fn log_startup_info(&self) {
        tracing::info!(
            "Role: {}, Rooms: {:?}, Allowed Roles: {:?}, Include ComponentB: {}",
            self.our_role,
            self.offered_rooms,
            self.allowed_roles,
            self.include_component_b
        );
    }
}
