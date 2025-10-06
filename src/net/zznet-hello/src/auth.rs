//! Authentication and authorization for HELLO protocol.
//!
//! This module defines authentication roles and implements authorization rules
//! for determining which roles can connect to which services and access which rooms.

// Re-export the application-level AuthRole from the centralized `zzping-auth` crate.
// This file keeps the local API stable while delegating the authoritative
// role and policy logic to `zzping-auth` (the application crate).

pub use zzping_auth::AuthRole;
