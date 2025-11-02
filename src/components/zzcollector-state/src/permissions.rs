//! Permissions for the zzcollector-state component.
//!
//! This module defines component-specific capabilities for collector state management.
//! Applications map global roles to these permissions at the composition root.

use serde::{Deserialize, Serialize};

/// Permissions for the zzcollector-state component.
///
/// These permissions control what operations a peer can perform within
/// the collector state management system.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CStatePermissions {
    /// Permission to send heartbeats (register as a collector).
    ///
    /// Typically granted to: collector
    /// Allows: Heartbeat messages
    pub can_send_heartbeat: bool,

    /// Permission to query the list of collectors.
    ///
    /// Typically granted to: client-admin, monitoring systems
    /// Allows: QueryCollectors messages
    pub can_query_collectors: bool,
}

impl CStatePermissions {
    /// Create new permissions with specified capabilities.
    pub fn new(can_send_heartbeat: bool, can_query_collectors: bool) -> Self {
        Self {
            can_send_heartbeat,
            can_query_collectors,
        }
    }

    /// Create permissions for a collector role (can send heartbeats, cannot query).
    pub fn for_collector() -> Self {
        Self {
            can_send_heartbeat: true,
            can_query_collectors: false,
        }
    }

    /// Create permissions for an admin role (can query collectors, cannot send heartbeats).
    pub fn for_admin() -> Self {
        Self {
            can_send_heartbeat: false,
            can_query_collectors: true,
        }
    }

    /// Create permissions with no capabilities (deny all).
    pub fn deny_all() -> Self {
        Self {
            can_send_heartbeat: false,
            can_query_collectors: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collector_permissions() {
        let perms = CStatePermissions::for_collector();
        assert!(perms.can_send_heartbeat);
        assert!(!perms.can_query_collectors);
    }

    #[test]
    fn test_admin_permissions() {
        let perms = CStatePermissions::for_admin();
        assert!(!perms.can_send_heartbeat);
        assert!(perms.can_query_collectors);
    }

    #[test]
    fn test_deny_all() {
        let perms = CStatePermissions::deny_all();
        assert!(!perms.can_send_heartbeat);
        assert!(!perms.can_query_collectors);
    }
}
