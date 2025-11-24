//! Implements the `HELLO` protocol for peer discovery and session negotiation.

mod actor;
mod connection_manager;
mod error;
mod handshake;
mod session_messages;

#[cfg(test)]
mod tests_integration;

#[cfg(test)]
mod tests_negotiation;

pub use actor::HelloActor;
pub use actor::HelloConfig;
pub use connection_manager::{ConnectionManager, HandleTransport};
