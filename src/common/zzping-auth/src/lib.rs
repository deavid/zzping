//! Application-layer authorization for ZZPing
//!
//! This crate provides authorization logic for ZZPing applications,
//! implementing allow-list based access control using peer identities
//! extracted from TLS certificates.

pub mod acl;
pub mod config;
pub mod error;
pub mod role;
