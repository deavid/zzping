//! Permissions for the zzpinger component.
//!
//! This module defines component-specific capabilities for ping operations.
//! Applications map global roles to these permissions at the composition root.

use serde::{Deserialize, Serialize};

/// Permissions for the zzpinger component.
///
/// These permissions control what operations a peer can perform within
/// the pinger component (controlling ping targets).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PingerPermissions {
    /// Permission to update ping targets.
    ///
    /// Typically granted to: collector (via database forwarding configuration updates)
    /// Allows: UpdateTargets messages
    pub can_update_targets: bool,
}

impl PingerPermissions {
    /// Create new permissions with specified capabilities.
    pub fn new(can_update_targets: bool) -> Self {
        Self { can_update_targets }
    }

    /// Create permissions for a collector role (can update targets).
    pub fn for_collector() -> Self {
        Self {
            can_update_targets: true,
        }
    }

    /// Create permissions with no capabilities (deny all).
    pub fn deny_all() -> Self {
        Self {
            can_update_targets: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collector_permissions() {
        let perms = PingerPermissions::for_collector();
        assert!(perms.can_update_targets);
    }

    #[test]
    fn test_deny_all() {
        let perms = PingerPermissions::deny_all();
        assert!(!perms.can_update_targets);
    }
}
