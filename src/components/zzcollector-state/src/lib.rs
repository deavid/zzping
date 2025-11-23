//! Manages the state and health of a collector instance.

mod actor;
mod builder;
mod config;
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

pub use actor::CStateActor;
pub use builder::CStateBuilder;
pub use config::CStateConfig;
pub use messages::*;
pub use permissions::CStatePermissions;
