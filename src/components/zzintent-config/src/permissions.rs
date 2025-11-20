//! IntentConfig Permissions
//!
//! Defines the permissions structure for the intent-config component.
//! This struct contains boolean flags for the specific capabilities
//! that a peer can have within this component.

/// Permissions for the intent-config component
///
/// This struct defines what a peer can do within the intent-config room.
/// Permissions are granted by the application based on the peer's global Role,
/// but the component itself is role-agnostic and only enforces these local permissions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct IntentConfigPermissions {
    /// Whether the peer can read the current configuration
    pub can_read_config: bool,
    /// Whether the peer can write/modify the configuration
    pub can_write_config: bool,
}

impl IntentConfigPermissions {
    /// Create a new permissions struct with the given capabilities
    pub fn new(can_read_config: bool, can_write_config: bool) -> Self {
        Self {
            can_read_config,
            can_write_config,
        }
    }

    /// Permissions that allow full access (read and write)
    pub fn full_access() -> Self {
        Self::new(true, true)
    }

    /// Permissions that allow only reading
    pub fn read_only() -> Self {
        Self::new(true, false)
    }

    /// Permissions that deny all access
    pub fn no_access() -> Self {
        Self::new(false, false)
    }
}
