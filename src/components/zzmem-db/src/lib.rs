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

pub mod actor;
pub mod builder;
pub mod config;
pub mod internal_messages;
pub mod messages;
pub mod network_actor;
pub mod network_manager;
pub mod network_messages;
pub mod permissions;
pub mod storage;

pub use permissions::MemDBPermissions;
