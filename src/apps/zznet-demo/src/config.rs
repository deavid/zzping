//! Configuration for the demo application.

use serde::{Deserialize, Serialize};
use std::clone::Clone;

/// Configuration for the demo application.
#[derive(Debug, Clone, Deserialize, Serialize)]
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
    /// Creates a new DemoAppConfig from role and peer address with defaults for testing.
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
