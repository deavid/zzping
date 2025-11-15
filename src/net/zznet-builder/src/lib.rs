//! ZZNet Application Builder - Complete Application Scaffolding Framework
//!
//! This crate provides a complete application framework for building ZZNet applications
//! with minimal boilerplate. It handles all the standard concerns: CLI parsing, logging,
//! configuration, TLS, runtime setup, and more.

pub mod builder;
mod cli;
mod logging;
mod runtime;
mod signals;
pub mod tls;
pub mod traits;
