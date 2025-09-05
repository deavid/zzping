//! Defines the structures and logic for role-based authentication and authorization.
//!
//! This module provides the building blocks for the gRPC authentication system.
//! It defines the shape of the JSON authentication token and the `UserIdentity`
//! struct that gets attached to each request after successful validation by the
//! `check_auth` interceptor.

use std::collections::HashSet;

/// A validated user identity, attached to a `tonic::Request`'s extensions.
///
/// This struct is created by the `check_auth` interceptor after successfully
/// decoding and validating a client's `AuthToken`. RPC handlers can then
/// extract this from the request extensions to perform fine-grained authorization
/// checks without needing to handle the raw token themselves.
#[derive(Debug, Clone)]
pub struct UserIdentity {
    /// The user's unique identifier, extracted from the `sub` field of the token.
    pub id: String,
    /// A `HashSet` of roles for efficient, O(1) role-checking.
    pub roles: HashSet<String>,
}

impl UserIdentity {
    /// Checks if the user identity includes a specific role.
    ///
    /// This is the primary method used by RPC handlers to authorize actions.
    ///
    /// # Example
    /// ```
    /// # use std::collections::HashSet;
    /// # use zzping_database::auth::UserIdentity;
    /// let identity = UserIdentity {
    ///     id: "test".to_string(),
    ///     roles: vec!["reader".to_string()].into_iter().collect(),
    /// };
    /// assert!(identity.has_role("reader"));
    /// assert!(!identity.has_role("collector"));
    /// ```
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.contains(role)
    }
}

/// A test-only helper to generate a valid Base64-encoded auth token.
///
/// This function is conditionally compiled and only available when the
/// `test-utils` feature is enabled. It simplifies the process of creating
/// valid tokens for integration tests.
#[cfg(feature = "test-utils")]
pub fn generate_test_token(sub: &str, roles: &[&str]) -> String {
    use zzping_lib::auth::AuthToken;

    let token = AuthToken {
        sub: sub.to_string(),
        roles: roles.iter().map(|s| s.to_string()).collect(),
    };
    let json = serde_json::to_string(&token).unwrap();
    base64::Engine::encode(&base64::engine::general_purpose::STANDARD, json)
}
