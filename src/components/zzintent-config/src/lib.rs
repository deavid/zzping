//! Intent-based configuration management for the zzping system.
//!
//! Implemented as a three-actor hierarchy: `IntentConfigActor`, `IntentConfigNetworkManager`,
//! and `IntentConfigNetworkActor`. Internal messages live in `internal_messages`.

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
