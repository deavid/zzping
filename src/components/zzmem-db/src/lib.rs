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
//! The component follows the ZZNet architecture where the same code handles
//! both sides of network communication, configured with different roles.
//!
//! ## Rooms
//!
//! This component communicates over the "memdb" room using `MemDBMessage` types.

/// Actor implementation for the MemDB component
pub mod actor;
/// Internal actor messages
pub mod messages;
/// Network protocol messages
pub mod network_messages;
/// Permission wrapper for authentication
pub mod permission_wrapper;
/// Permission definitions for MemDB
pub mod permissions;
/// Role-based configuration
pub mod role;
/// Storage backend for Database role
pub mod storage;
