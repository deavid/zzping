//! # zzpinger Component
//!
//! Manages ICMP ping operations for multiple network targets with configurable rates and timeouts.
//! Submits ping results to `zzmem-db` for storage and analysis, enabling network monitoring.
//!
//! The design emphasizes testability and reliability, ensuring no real network operations occur during testing.
//! Uses Actix actors for concurrent target management and tokio tasks for rate-limited ping loops.

pub mod actor;
pub mod api;
pub mod builder;
pub mod error;
pub mod messages;
pub mod network_actor;
pub mod network_manager;
pub mod network_messages;
pub mod permissions;
pub mod pinger;
