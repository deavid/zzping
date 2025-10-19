//! The zzintent-config crate provides intent-based configuration management for the zzping system.

pub mod actor;
pub mod api;
pub mod builder;
pub mod messages;
pub mod network_messages;
/// A wrapper around an ApplicationRole to be used by the IntentConfig component.
pub mod permission_wrapper;
pub mod permissions;
pub mod role;

/// The public API for the IntentConfig component.
#[cfg(test)]
mod api_tests;
mod broadcast_timeout_tests;
#[cfg(test)]
mod builder_tests;
#[cfg(test)]
mod test_integration;
#[cfg(test)]
mod test_utils;
