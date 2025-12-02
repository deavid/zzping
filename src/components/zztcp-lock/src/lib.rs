//! A component that provides a TCP-based local mastership lock.
//!
//! This crate contains an actor, `TcpLockActor`, which attempts to acquire a lock
//! using a configurable strategy:
//!
//! - **TCP Mode (Production):** Binds to a configured TCP port. A successful bind
//!   indicates that this process holds the lock, ensuring that only one instance
//!   of the service can run on a host at a time.
//!
//! - **Memory Mode (Testing):** Uses an in-memory atomic registry. This enables
//!   deterministic testing without OS-level port binding issues or `TIME_WAIT` delays.
//!
//! The actor uses a message-driven loop (`CheckLock`) instead of automatic intervals,
//! allowing tests to force immediate lock checks in `tokio::time::pause()` environments.

pub mod actor;
pub mod backend;
pub mod config;
pub mod messages;
