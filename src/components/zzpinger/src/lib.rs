//! # zzpinger Component
//!
//! Manages ICMP ping operations for multiple network targets with configurable rates and timeouts.
//! Submits ping results to `zzmem-db` for storage and analysis, enabling network monitoring.
//!
//! The design emphasizes testability and reliability, ensuring no real network operations occur during testing.
//! Uses Actix actors for concurrent target management and tokio tasks for rate-limited ping loops.

/// Actix actors that manage pinger lifecycle and messages.
pub mod actor;
/// Public API types and helpers for integrating the component.
pub mod api;
/// Configuration and builder utilities for constructing pingers.
pub mod builder;
/// Error types used by the component.
pub mod error;
/// Message types exchanged between actors.
pub mod messages;
/// Network manager for Router integration.
pub mod network_manager;
/// Network protocol messages for remote communication.
pub mod network_messages;
/// Core ping logic and backend abstractions.
pub mod pinger;
/// Translator actor for per-peer message translation.
pub mod translator_actor;
