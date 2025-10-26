//! Generic role trait for application-specific authorization.
//!
//! This module defines the `ApplicationRole` trait that applications must
//! implement to use zznet-auth's authorization system.

use crate::error::AuthError;
use serde::{Serialize, de::DeserializeOwned};
use std::fmt::Debug;
use std::hash::Hash;

/// Trait for application-specific roles that can be used with zznet-auth.
///
/// Applications should implement this trait for their custom role types.
pub trait ApplicationRole:
    Clone
    + Copy
    + Debug
    + PartialEq
    + Eq
    + Hash
    + Serialize
    + DeserializeOwned
    + Send
    + Sync
    + Unpin
    + 'static
{
    /// Create a role from a common name (CN) in a certificate.
    fn from_cn(cn: &str) -> Result<Self, AuthError>;

    /// Get the string representation of the role.
    fn as_str(&self) -> &'static str;

    /// Check if this role can connect to another role.
    fn can_connect_to(&self, target: &Self) -> bool;

    /// Check if this role can access a room.
    fn can_access_room(&self, room_name: &str) -> bool;

    /// Return the role variant that allows receiving configuration updates.
    /// Returns `None` if no such role exists for this `ApplicationRole` type.
    #[deprecated(note="This does not belong to zznet-* crates, this is a zzintent-config specific")]
    fn receive_config_updates_role() -> Option<Self> {
        None // Default implementation returns None
    }
}
