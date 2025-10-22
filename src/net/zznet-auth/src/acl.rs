//! Access Control List (ACL) implementation for ZZPing authorization.
//!
//! This module provides role-based authorization using allow-lists.
//! Roles are determined from HELLO protocol messages (primary) and validated
//! against TLS certificates (if present).

use std::collections::HashSet;
use std::marker::PhantomData;

use crate::role::ApplicationRole;

/// Generic type alias for authorizer closures with custom role types.
///
/// The authorizer receives an AuthContext with:
/// - hello_role_str: Role claimed in HELLO message (PRIMARY source)
/// - peer_identity: Optional TLS identity for validation
pub type GenericAuthorizer<R> =
    Box<dyn Fn(&zznet_api::types::AuthContext) -> Option<R> + Send + Sync>;

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

    // TODO: Update these methods to work with AuthContext instead of PeerIdentity
    // For now, commented out since they aren't used yet and the API is changing

    /*
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
    pub fn to_authorizer(self) -> GenericAuthorizer<R> {
        self.to_generic_authorizer()
    }

    /// Create a generic Authorizer closure suitable for ConnectionManager.
    ///
    /// This is the recommended way to integrate AclManager with ConnectionManager.
    /// The returned closure captures the AclManager and can be passed to
    /// `ConnectionManager::new_with_acl()`.
    pub fn to_generic_authorizer(self) -> GenericAuthorizer<R> {
        Box::new(move |identity| self.authorize_peer_option(identity))
    }
    */
}

/// Create a default authorizer that implements the correct authentication model:
/// HELLO role is PRIMARY, TLS certificate VALIDATES (if present).
///
/// This is a simple authorizer suitable for basic deployments. For production systems
/// with complex access control requirements, consider using `AclManager::to_authorizer()`
/// which supports allow-lists and per-role permissions.
///
/// # Parameters
/// - `allow_insecure_tcp`: when true, accepts HELLO role without TLS validation
///
/// # Security Model
/// 1. Parse role from HELLO message (primary source of identity - **mandatory**)
/// 2. If TLS is present, verify HELLO role matches certificate CN
/// 3. If TLS is absent, check `allow_insecure_tcp` flag before accepting
///
/// # Important
/// There is NO "default role". The role **always** comes from the HELLO message.
/// The peer must send a valid role in the HELLO handshake.
pub fn create_default_authorizer<R: ApplicationRole>(
    allow_insecure_tcp: bool,
) -> GenericAuthorizer<R> {
    Box::new(move |auth_ctx: &zznet_api::types::AuthContext| {
        // Step 1: Parse HELLO role (always required - this is the source of truth)
        let hello_role = match R::from_cn(&auth_ctx.hello_role_str) {
            Ok(role) => role,
            Err(e) => {
                tracing::warn!(
                    "Failed to parse HELLO role '{}': {:?}",
                    auth_ctx.hello_role_str,
                    e
                );
                return None;
            }
        };

        // Step 2: If TLS exists, validate HELLO against certificate
        if let Some(ref peer_id) = auth_ctx.peer_identity {
            let cert_role = match R::from_cn(&peer_id.common_name) {
                Ok(role) => role,
                Err(e) => {
                    tracing::error!(
                        "Failed to parse certificate CN '{}': {:?}",
                        peer_id.common_name,
                        e
                    );
                    return None;
                }
            };

            // SECURITY: HELLO claim must match TLS certificate
            if hello_role.as_str() != cert_role.as_str() {
                tracing::error!(
                    "!!! SECURITY VIOLATION !!!: HELLO claimed {:?} but certificate says {:?}",
                    hello_role.as_str(),
                    cert_role.as_str()
                );
                return None;
            }

            tracing::info!(
                "Authorized: HELLO={}, validated by cert ({})",
                hello_role.as_str(),
                peer_id.full_identity()
            );
            return Some(hello_role);
        }

        // Step 3: No TLS - check insecure mode
        if !allow_insecure_tcp {
            tracing::error!("Connection without TLS rejected (insecure mode disabled)");
            return None;
        }

        // INSECURE: Trust HELLO claim without validation
        tracing::warn!(
            "⚠️  INSECURE MODE: Trusting HELLO claim {} without TLS validation",
            hello_role.as_str()
        );
        Some(hello_role)
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

    fn mk_auth_ctx(
        hello_role: &str,
        peer_id: Option<zznet_api::types::PeerIdentity>,
    ) -> zznet_api::types::AuthContext {
        zznet_api::types::AuthContext {
            hello_role_str: hello_role.to_string(),
            peer_identity: peer_id,
        }
    }

    #[test]
    fn test_default_authorizer_with_matching_tls() {
        let auth = create_default_authorizer::<TestRole>(false);
        let ctx = mk_auth_ctx("collector", Some(mk_identity("collector", "root")));
        let role = auth(&ctx);
        assert_eq!(role, Some(TestRole::Collector));
    }

    #[test]
    fn test_default_authorizer_insecure_tcp_mode() {
        let auth = create_default_authorizer::<TestRole>(true);
        let ctx = mk_auth_ctx("database", None); // No TLS identity
        let role = auth(&ctx);
        assert_eq!(role, Some(TestRole::Database));
    }

    #[test]
    fn test_default_authorizer_rejects_mismatched_roles() {
        let auth = create_default_authorizer::<TestRole>(false);
        // HELLO says "collector" but cert says "database"
        let ctx = mk_auth_ctx("collector", Some(mk_identity("database", "root")));
        let role = auth(&ctx);
        assert_eq!(role, None); // Security violation!
    }

    #[test]
    fn test_default_authorizer_rejects_tcp_without_insecure_flag() {
        let auth = create_default_authorizer::<TestRole>(false);
        let ctx = mk_auth_ctx("collector", None); // No TLS, insecure mode disabled
        let role = auth(&ctx);
        assert_eq!(role, None); // TLS required
    }
}
