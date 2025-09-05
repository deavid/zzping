//! Authentication-related structures and logic.
//!
//! This module provides the building blocks for authentication in the zzping ecosystem.

/// The structure of the JSON Web Token (JWT) used for authentication.
///
/// This token is expected to be Base64-encoded and sent by the client in the
/// `Authorization` header. It contains the subject (user/client ID) and a list
/// of roles that grant specific permissions.
#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct AuthToken {
    /// The subject of the token, typically a unique identifier for the client
    /// (e.g., a collector's hostname or a GUI user's ID).
    pub sub: String,
    /// A list of roles assigned to the subject. These roles are used by RPC
    /// handlers to make authorization decisions.
    pub roles: Vec<String>,
}
