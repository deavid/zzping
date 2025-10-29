//! The `zzcollector-state` component manages the state and health of a collector instance.
//!
//! It is responsible for:
//! - Maintaining a unique collector identity.
//! - Periodically sending heartbeat messages to a database role to signal liveness.
//! - Aggregating health metrics from other components.
//! - Tracking active collectors when configured in a `Database` role.
//!
//! ## Architecture (Three-Actor Pattern)
//!
//! This component follows the three-actor pattern for clean separation of concerns:
//!
//! - **CStateActor** (MainActor): Pure business logic - collector registry, health tracking
//! - **CStateNetworkManager**: Peer lifecycle orchestration, spawns NetworkActors
//! - **CStateNetworkActor**: Per-peer protocol translation via Room<CStateMessage>

pub mod actor;
pub mod api;
pub mod builder;
pub mod config;
pub mod internal_messages;
pub mod messages;
pub mod network_actor;
pub mod network_manager;
pub mod network_messages;
pub mod state;

#[cfg(test)]
mod tests;
