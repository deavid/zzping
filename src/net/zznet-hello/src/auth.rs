//! Authentication and authorization for HELLO protocol.
//!
//! This module defines authentication roles and implements authorization rules
//! for determining which roles can connect to which services and access which rooms.

// Re-export the generic ApplicationRole trait from `zznet-auth`.
// Applications implement this trait with their own role types.
// The protocol layer works with this trait generically.

pub use zznet_auth::ApplicationRole;
// Do NOT re-export or depend on application-specific role enums here.
// Protocol must be application-agnostic and exchange role identifiers as strings.
