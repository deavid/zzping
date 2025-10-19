//! Generic role trait for application-specific authorization.
//!
//! This module defines the `ApplicationRole` trait that applications must
//! implement to use zznet-auth's authorization system.

use std::fmt::Debug;

use serde::{Deserialize, Serialize};

/// Trait for application-specific roles that can be used with zznet-auth.
///
/// Applications should implement this trait for their custom role types.
pub trait ApplicationRole:
    Clone
    + Copy
    + PartialEq
    + Eq
    + Serialize
    + for<'de> Deserialize<'de>
    + Send
    + Sync
    + 'static
    + Unpin
    + Debug
{
    /// Parse role from certificate Common Name
    fn from_cn(cn: &str) -> Result<Self, crate::error::AuthError>;

    /// Convert role to certificate CN string
    fn as_str(&self) -> &'static str;

    /// Check if this role can connect to target role
    fn can_connect_to(&self, target: &Self) -> bool;

    /// Check if this role can access a specific room
    fn can_access_room(&self, room_name: &str) -> bool;
}

// No concrete AuthRole defined here. Applications must define their own
// role enum implementing `ApplicationRole` and provide any convenience
// aliases (for example `AclManagerDefault`) from the application crate.
