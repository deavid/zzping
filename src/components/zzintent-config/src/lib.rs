//! # Security
//!
//! **Authentication:** Peer identities are verified using TLS certificates.
//! The peer's role is resolved from the certificate's CN (Common Name) field.
//!
//! **Authorization:** Configuration changes via `RequestConfigChange` are
//! only accepted from peers with the `ClientAdmin` role. Other roles are
//! silently rejected.
//!
//! **Role-Based Filtering:** The Database only sends `ConfigUpdate` messages
//! to peers with the `Collector` role. AdminClients and other roles do not
//! receive configuration updates.
//!
//! **Audit Logging:** All configuration changes are logged with the peer's
//! full identity (username@role format) for audit trails.
//!
//! ## Security Implementation Details
//!
//! - Peer roles are stored in `PeerSession` and resolved during connection handshake
//! - Authorization checks use `SessionManager.get_peer_role()` to verify permissions
//! - Config updates are filtered by role before sending to avoid information leakage
//! - All security decisions are logged with peer identity information

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
#[cfg(test)]
mod builder_tests;
#[cfg(test)]
mod test_integration;
