//! ZZNet Application Builder - Complete Application Scaffolding Framework
//!
//! This crate provides a complete application framework for building ZZNet applications
//! with minimal boilerplate. It handles all the standard concerns: CLI parsing, logging,
//! configuration, TLS, runtime setup, and more.

pub mod builder;
pub mod cli;
mod error;
pub mod logging;
pub mod runtime;
pub mod signals;
pub mod tls;
pub mod traits;
