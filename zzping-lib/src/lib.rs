//! A shared library for the zzping project.
//!
//! This crate contains common data structures, protocols, and logic
//! used by the various services in the zzping ecosystem. The goal is
//! to centralize this code to avoid duplication and improve maintainability.

pub mod auth;
pub mod chunked_v1;
pub mod protocol;
