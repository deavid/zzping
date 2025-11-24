//! Intent-based configuration management for the zzping system.

mod actor;
mod builder;
mod config;
mod events;
mod internal_messages;
mod messages;
mod network_actor;
mod network_messages;
mod permissions;
mod spec;

#[cfg(test)]
mod tests;

pub use actor::IntentConfigActor;
pub use builder::IntentConfigBuilder;
pub use config::IntentConfigConfig;
pub use messages::*;
pub use permissions::IntentConfigPermissions;
