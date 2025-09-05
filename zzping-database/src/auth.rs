//! Defines the structures and logic for role-based authentication and authorization.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Represents the structure of the JSON token provided by the client.
#[derive(Debug, Deserialize, Serialize)]
pub struct AuthToken {
    /// The subject or user ID.
    pub sub: String,
    /// A list of roles assigned to the user.
    pub roles: Vec<String>,
}

/// Represents the validated user identity attached to each request.
/// This is created by the `check_auth` interceptor and used by RPC handlers.
#[derive(Debug, Clone)]
pub struct UserIdentity {
    /// The user's unique identifier.
    pub id: String,
    /// A set of roles for efficient lookup.
    pub roles: HashSet<String>,
}

impl UserIdentity {
    /// Checks if the user has a specific role.
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.contains(role)
    }
}

// Helper for tests to generate tokens. This will be conditionally compiled.
#[cfg(feature = "test-utils")]
pub fn generate_test_token(sub: &str, roles: &[&str]) -> String {
    let token = AuthToken {
        sub: sub.to_string(),
        roles: roles.iter().map(|s| s.to_string()).collect(),
    };
    let json = serde_json::to_string(&token).unwrap();
    base64::Engine::encode(&base64::engine::general_purpose::STANDARD, json)
}
