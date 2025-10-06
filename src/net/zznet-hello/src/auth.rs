//! Authentication and authorization for HELLO protocol.
//!
//! This module defines authentication roles and implements authorization rules
//! for determining which roles can connect to which services and access which rooms.

// Re-export the application-level AuthRole from the centralized `zzping-auth` crate.
// This keeps the protocol layer working with concrete roles while the session layer
// is generic. The protocol layer needs concrete roles for serialization and protocol logic.

pub use zznet_auth::ApplicationRole;
// Do NOT re-export or depend on application-specific role enums here.
// Protocol must be application-agnostic and exchange role identifiers as strings.
