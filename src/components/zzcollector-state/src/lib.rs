//! The `zzcollector-state` component manages the state and health of a collector instance.
//!
//! It is responsible for:
//! - Maintaining a unique collector identity.
//! - Periodically sending heartbeat messages to a database role to signal liveness.
//! - Aggregating health metrics from other components.
//! - Tracking active collectors when configured in a `Database` role.

#![deny(
    dead_code,
    unused_variables,
    // missing_docs
)]

pub mod actor;
pub mod api;
pub mod builder;
pub mod messages;
pub mod network_messages;
/// Permissions module for the component (defines `CStatePermission`).
pub mod permissions;
pub mod role;
pub mod state;

#[cfg(test)]
mod tests;
