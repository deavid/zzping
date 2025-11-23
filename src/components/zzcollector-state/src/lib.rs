//! Manages the state and health of a collector instance.

pub mod actor;
pub mod api;
pub mod builder;
pub mod config;
pub mod events;
pub mod internal_messages;
pub mod messages;
pub mod network_actor;
pub mod network_manager;
pub mod network_messages;
pub mod permissions;
pub mod state;

#[cfg(test)]
mod tests;
