//! ZZNet Runtime Framework
//!
//! This crate provides runtime helpers—logging, signals, TLS, harness integration—for
//! building ZZNet applications with minimal boilerplate.
pub mod harness;
mod logging;
mod runtime;
mod signals;
pub mod tls;
pub mod traits;
