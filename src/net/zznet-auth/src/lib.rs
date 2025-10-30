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
//! #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

pub mod acl;
pub mod config;
pub mod error;
#[cfg(any(test, feature = "test-utils"))]
pub mod mock;
pub mod role;
