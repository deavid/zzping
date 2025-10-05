//! # ⚠️ SECURITY WARNING (Phase 2 Implementation)
//!
//! **This component is NOT production-ready and lacks authentication/authorization.**
//!
//! ## Current Security Status
//! - ✅ Network communication: Not yet implemented (Phase 3)
//! - ❌ Authentication: Not implemented
//! - ❌ Authorization: Not implemented
//! - ❌ Audit logging: Not implemented
//!
//! ## What This Means
//! ANY network peer can send `RequestConfigChange` messages to change system configuration.
//! This is acceptable for:
//! - Local testing environments
//! - Development and integration testing
//!
//! ## Production Requirements
//! Before production deployment, Phase 4 (Auth Integration) MUST be completed:
//! - Peer identity extraction from TLS certificates
//! - Peer role filtering (Database should only send to Collectors, not AdminClients)
//! - ACL-based authorization for `RequestConfigChange`
//! - Audit logging for all configuration changes
//! - Secure defaults (deny by default)
//!
//! ## Deployment Guard
//! Phase 3 will add a startup check that prevents production deployment
//! without auth integration.
//!
//! For implementation details of the security gap, see the docstrings on:
//! - `IntentConfigActor` (actor.rs)
//! - `IntentConfigMessage::RequestConfigChange` (network_messages.rs)

pub mod actor;
pub mod api;
pub mod builder;
pub mod messages;
pub mod network_messages;
pub mod role;

#[cfg(test)]
mod api_tests;
#[cfg(test)]
mod builder_tests;
#[cfg(test)]
mod test_integration;
