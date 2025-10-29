//! # zzmem-db
//!
//! In-memory ping database component for ZZPing.
//!
//! This component stores ping results in memory and provides query capabilities.
//! It operates in two roles:
//! - **Collector role**: Buffers ping results and sends batches to database
//! - **Database role**: Receives batches and provides query interface
//!
//! ## Architecture
//!
//! The component follows the three-actor pattern:
//! - **MainActor** (MemDBActor): Pure business logic, zero network dependencies
//! - **NetworkManager**: Peer lifecycle orchestration and message routing
//! - **NetworkActor**: Per-peer protocol translation
//!
//! ## Rooms
//!
//! This component communicates over the "memdb" room using `MemDBMessage` types.

/// Actor implementation for the MemDB component (MainActor)
pub mod actor;
/// Builder for creating MemDBActor with three-actor pattern
pub mod builder;
/// Fine-grained configuration (replaces role enum)
pub mod config;
/// Internal messages for three-actor communication
pub mod internal_messages;
/// Internal actor messages (legacy, API messages)
pub mod messages;
/// NetworkActor - per-peer protocol translation
pub mod network_actor;
/// NetworkManager - peer lifecycle and routing
pub mod network_manager;
/// Network protocol messages
pub mod network_messages;
/// Storage backend for Database role
pub mod storage;
