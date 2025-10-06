//! Application-layer authorization for ZZPing
//!
//! This crate provides authorization logic for applications,
//! implementing allow-list based access control using peer identities
//! extracted from TLS certificates.
//!
//! ## Generic Role Support
//!
//! This crate supports custom role types through the `ApplicationRole` trait.
//! Applications can implement their own roles:
//!
//! ```rust
//! use zznet_auth::{ApplicationRole, AclManager};
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
//! enum MyRole { Admin, User, Guest }
//!
//! impl ApplicationRole for MyRole {
//!     fn from_cn(cn: &str) -> Result<Self, zznet_auth::error::AuthError> {
//!         match cn {
//!             "admin" => Ok(MyRole::Admin),
//!             "user" => Ok(MyRole::User),
//!             "guest" => Ok(MyRole::Guest),
//!             _ => Err(zznet_auth::error::AuthError::UnknownRole(cn.to_string())),
//!         }
//!     }
//!
//!     fn as_str(&self) -> &'static str {
//!         match self {
//!             MyRole::Admin => "admin",
//!             MyRole::User => "user",
//!             MyRole::Guest => "guest",
//!         }
//!     }
//!
//!     fn can_connect_to(&self, target: &Self) -> bool {
//!         // Custom connection logic
//!         true
//!     }
//!
//!     fn can_access_room(&self, room_name: &str) -> bool {
//!         // Custom room access logic
//!         matches!(self, MyRole::Admin)
//!     }
//! }
//!
//! // Use with AclManager
//! let acl = AclManager::<MyRole>::new();
//! ```
//!
//! ## Mock Roles for Testing
//!
//! For testing purposes, you can use the provided `MockRole` when the `test-utils` feature is enabled:
//!
//! ```rust,ignore
//! use zznet_auth::{mock::MockRole, AclManager};
//!
//! let acl = AclManager::<MockRole>::new();
//! ```

/// Access-control list utilities for allow-lists.
pub mod acl;

/// Configuration helpers for authorization components.
pub mod config;

/// Error types used by the authorization subsystem.
pub mod error;

/// Mock roles for testing.
#[cfg(any(test, feature = "test-utils"))]
pub mod mock;

/// Application-level role definitions and conversions.
pub mod role;

// Re-export key types for convenience
pub use acl::{AclManager, GenericAuthorizer};
pub use error::AuthError;
pub use role::ApplicationRole;
