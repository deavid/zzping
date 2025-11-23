//! Permissions module for MemDB component.
//!
//! Defines component-specific permissions that control what operations
//! a peer can perform within the MemDB component. These permissions are
//! derived from the peer's global Role but are enforced locally by the component.

/// Permissions for MemDB component operations.
///
/// These permissions control what a peer can do within the MemDB component.
/// They are granted based on the peer's global Role but are enforced
/// at the component level, keeping the component role-agnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemDBPermissions {
    /// Whether the peer can submit batches of ping results
    pub can_submit_batch: bool,

    /// Whether the peer can query stored data
    pub can_query: bool,

    /// Whether the peer can receive batch submissions (for database role)
    pub can_receive_batches: bool,
}

impl MemDBPermissions {
    /// Create a new permissions struct with the given capabilities.
    pub fn new(can_submit_batch: bool, can_query: bool, can_receive_batches: bool) -> Self {
        Self {
            can_submit_batch,
            can_query,
            can_receive_batches,
        }
    }

    /// Permissions for Collector role: can submit batches, cannot query or receive.
    pub fn collector() -> Self {
        Self::new(true, false, false)
    }

    /// Permissions for Database role: can receive batches and query, cannot submit.
    pub fn database() -> Self {
        Self::new(false, true, true)
    }

    /// Permissions for Admin role: full access to all operations.
    pub fn admin() -> Self {
        Self::new(true, true, true)
    }

    /// No permissions (default for unknown or unauthorized roles).
    pub fn none() -> Self {
        Self::new(false, false, false)
    }
}

impl Default for MemDBPermissions {
    /// Default permissions deny all operations.
    fn default() -> Self {
        Self::none()
    }
}
