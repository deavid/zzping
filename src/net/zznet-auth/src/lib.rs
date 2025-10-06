//! Application-layer authorization for ZZPing
//!
//! This crate provides authorization logic for ZZPing applications,
//! implementing allow-list based access control using peer identities
//! extracted from TLS certificates.

/// Access-control list utilities for allow-lists.
pub mod acl;

/// Configuration helpers for authorization components.
pub mod config;

/// Error types used by the authorization subsystem.
pub mod error;

/// Application-level role definitions and conversions.
pub mod role;
