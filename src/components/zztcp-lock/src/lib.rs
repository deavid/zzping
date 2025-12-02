//! A component that provides a TCP-based local mastership lock.
//!
//! This crate contains an actor, `TcpLockActor`, which attempts to bind to a
//! configured TCP port. A successful bind indicates that this process holds the
//! lock, ensuring that only one instance of the service can run on a host at a time.

pub mod actor;
pub mod messages;
