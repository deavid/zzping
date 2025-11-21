//! Implements the `HELLO` protocol for peer discovery and session negotiation.

pub mod actor;
pub mod connection_manager;
mod error;
mod handshake;
mod session_messages;

#[cfg(test)]
mod integration_tests;
