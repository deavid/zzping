//! Manages the state and health of a collector instance.

mod actor;
pub mod builder;
pub mod config;
mod events;
mod internal_messages;
mod messages;
mod network_actor;
mod network_manager;
mod network_messages;
mod permissions;
mod state;

#[cfg(test)]
mod tests;
