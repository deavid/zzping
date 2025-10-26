//! The zzintent-config crate provides intent-based configuration management for the zzping system.

pub mod actor;
pub mod api;
pub mod builder;
pub mod messages;
pub mod network_messages;
pub mod permissions;
pub mod role;
pub mod room_handler;

/// The public API for the IntentConfig component.
#[cfg(test)]
mod api_tests;
#[cfg(test)]
mod builder_tests;
#[cfg(test)]
mod test_integration;
#[cfg(test)]
mod test_utils;
