//! Access Control List (ACL) implementation for ZZPing authorization.
//!
//! This module provides role-based authorization using allow-lists.
//! Roles are determined from TLS certificate Common Names (CN=role format).

use std::collections::HashSet;
use std::marker::PhantomData;

use crate::role::ApplicationRole;

/// Generic type alias for authorizer closures with custom role types.
pub type GenericAuthorizer<R> =
    Box<dyn Fn(&zznet_api::types::PeerIdentity) -> Option<R> + Send + Sync>;

// Note: No application-specific aliases are exported here. Applications
// should provide their own concrete role type and (optionally) a
// convenience type alias in their application crate, e.g.:
//
// pub type AclManagerDefault = zznet_auth::acl::AclManager<MyAuthRole>;

/// Access Control List manager for authorization decisions.
///
/// This implements allow-list based authorization where only explicitly
/// allowed identities can access the system.
#[derive(Debug, Clone)]
pub struct AclManager<R: ApplicationRole> {
    /// Set of allowed canonical peer strings (e.g. "alice@collector" or "collector").
    allowed_peers: HashSet<String>,
    /// Whether to trust HELLO messages in insecure (non-TLS) mode.
    insecure_trust_hello: bool,
    /// Phantom data to make the struct generic over R
    _phantom: PhantomData<R>,
}

impl<R: ApplicationRole> AclManager<R> {
    /// Creates a new ACL manager with no allowed users.
    pub fn new() -> Self {
        Self {
            allowed_peers: HashSet::new(),
            insecure_trust_hello: false,
            _phantom: PhantomData,
        }
    }

    /// Creates an ACL manager from a set of allowed usernames.
    pub fn with_allowed_peers(allowed_peers: HashSet<String>) -> Self {
        Self {
            allowed_peers,
            insecure_trust_hello: false,
            _phantom: PhantomData,
        }
    }

    /// Creates an ACL manager from a set of allowed peers and security flag.
    pub fn with_allowed_peers_and_insecure(
        allowed_peers: HashSet<String>,
        insecure_trust_hello: bool,
    ) -> Self {
        Self {
            allowed_peers,
            insecure_trust_hello,
            _phantom: PhantomData,
        }
    }

    /// Checks if a username (from SAN) is authorized to access the system.
    ///
    /// This root check is intentionally simple and mainly used by older APIs.
    pub fn is_authorized(&self, username: &str) -> bool {
        self.allowed_peers.contains(username)
    }

    /// Authorize a peer by its full PeerIdentity.
    ///
    /// Matching rules:
    /// - If allowed_peers contains "<username>@<role>" that equals the
    ///   canonical identity, it's allowed.
    /// - If allowed_peers contains "<role>", any user with that role is allowed.
    ///
    /// Returns the resolved role on success.
    pub fn authorize_peer(
        &self,
        identity: &zznet_api::types::PeerIdentity,
    ) -> Result<R, crate::error::AuthError> {
        tracing::debug!(peer = %identity.full_identity(), "authorize_peer called");

        let canonical = format!("{}@{}", identity.san_username, identity.common_name);
        if self.allowed_peers.contains(&canonical) {
            tracing::info!(peer = %identity.full_identity(), "authorized by canonical match");
            return R::from_cn(&identity.common_name);
        }

        // Try role-only match
        if self.allowed_peers.contains(&identity.common_name) {
            tracing::info!(peer = %identity.full_identity(), role = %identity.common_name, "authorized by role-only match");
            return R::from_cn(&identity.common_name);
        }

        tracing::warn!(peer = %identity.full_identity(), "authorization denied");
        Err(crate::error::AuthError::IdentityNotAllowed(
            identity.full_identity(),
        ))
    }

    /// Adds a username to the allow-list.
    pub fn allow_user(&mut self, username: &str) {
        self.allowed_peers.insert(username.to_string());
    }

    /// Removes a username from the allow-list.
    pub fn deny_user(&mut self, username: &str) {
        self.allowed_peers.remove(username);
    }

    /// Returns the set of allowed users.
    pub fn allowed_peers(&self) -> &HashSet<String> {
        &self.allowed_peers
    }

    /// Returns whether insecure trust mode is enabled.
    pub fn is_insecure_mode(&self) -> bool {
        self.insecure_trust_hello
    }

    /// Bridge function for ConnectionManager's Authorizer type.
    ///
    /// This wraps `authorize_peer` to return `Option<R>` instead of `Result`,
    /// which matches the signature expected by ConnectionManager's authorizer closure.
    ///
    /// Returns `Some(role)` if authorized, `None` if denied or error.
    pub fn authorize_peer_option(&self, identity: &zznet_api::types::PeerIdentity) -> Option<R> {
        self.authorize_peer(identity).ok()
    }

    /// Create an authorizer closure suitable for ConnectionManager.
    ///
    /// This is a backwards-compatible alias for `to_generic_authorizer()`.
    /// Returns a closure that can be passed to `ConnectionManager::new_with_acl()`.
    ///
    /// # Example
    /// ```ignore
    /// let acl = AclManager::<MyRole>::with_allowed_peers(allowed_peers);
    /// let authorizer = acl.to_authorizer();
    /// let conn_mgr = ConnectionManager::new_with_acl(rooms, Some((authorizer, false)));
    /// ```
    pub fn to_authorizer(self) -> GenericAuthorizer<R> {
        self.to_generic_authorizer()
    }

    /// Create a generic Authorizer closure suitable for ConnectionManager.
    ///
    /// This is the recommended way to integrate AclManager with ConnectionManager.
    /// The returned closure captures the AclManager and can be passed to
    /// `ConnectionManager::new_with_acl()`.
    ///
    /// # Example
    /// ```ignore
    /// let acl = AclManager::<MyRole>::with_allowed_peers(allowed_peers);
    /// let authorizer = acl.to_generic_authorizer();
    /// let conn_mgr = ConnectionManager::new_with_acl(rooms, Some((authorizer, false)));
    /// ```
    pub fn to_generic_authorizer(self) -> GenericAuthorizer<R> {
        Box::new(move |identity| self.authorize_peer_option(identity))
    }
}

/// Create a default authorizer that implements the common pattern used by
/// applications: parse role from certificate CN, and optionally accept
/// plain-TCP fallback as a specific default role.
///
/// - `allow_plain_tcp`: when true, treat `peer_identity.common_name == "plain-tcp"` as allowed
/// - `default_role_for_plain`: role to return when plain-tcp is allowed
pub fn create_default_authorizer<R: ApplicationRole>(
    allow_plain_tcp: bool,
    default_role_for_plain: Option<R>,
) -> GenericAuthorizer<R> {
    Box::new(move |peer_identity: &zznet_api::types::PeerIdentity| {
        // Plain-TCP handling
        if allow_plain_tcp && peer_identity.common_name == "plain-tcp" {
            return default_role_for_plain;
        }

        // Try parsing CN to role
        R::from_cn(&peer_identity.common_name).ok()
    })
}

impl<R: ApplicationRole> Default for AclManager<R> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    enum TestRole {
        Collector,
        Database,
    }

    impl crate::role::ApplicationRole for TestRole {
        fn from_cn(cn: &str) -> Result<Self, crate::error::AuthError> {
            match cn {
                "collector" => Ok(TestRole::Collector),
                "database" => Ok(TestRole::Database),
                _ => Err(crate::error::AuthError::UnknownRole(cn.to_string())),
            }
        }

        fn as_str(&self) -> &'static str {
            match self {
                TestRole::Collector => "collector",
                TestRole::Database => "database",
            }
        }

        fn can_connect_to(&self, _target: &Self) -> bool {
            true
        }

        fn can_access_room(&self, _room_name: &str) -> bool {
            true
        }
    }

    fn mk_identity(cn: &str, username: &str) -> zznet_api::types::PeerIdentity {
        zznet_api::types::PeerIdentity {
            common_name: cn.to_string(),
            san_username: username.to_string(),
            peer_addr: "127.0.0.1:0".to_string(),
        }
    }

    #[test]
    fn test_default_authorizer_parses_known_role() {
        let auth = create_default_authorizer::<TestRole>(false, None);
        let id = mk_identity("collector", "root");
        let role = auth(&id);
        assert_eq!(role, Some(TestRole::Collector));
    }

    #[test]
    fn test_default_authorizer_plain_tcp_fallback() {
        let auth = create_default_authorizer::<TestRole>(true, Some(TestRole::Database));
        let id = mk_identity("plain-tcp", "root");
        let role = auth(&id);
        assert_eq!(role, Some(TestRole::Database));
    }

    #[test]
    fn test_default_authorizer_rejects_unknown_cn() {
        let auth = create_default_authorizer::<TestRole>(false, None);
        let id = mk_identity("unknown-role", "bob");
        let role = auth(&id);
        assert_eq!(role, None);
    }
}
