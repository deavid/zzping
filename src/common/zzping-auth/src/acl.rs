//! Access Control List (ACL) implementation for ZZPing authorization.
//!
//! This module provides role-based authorization using allow-list access control.
//! Roles are determined from TLS certificate Common Names (CN=role format).

use std::collections::HashSet;

use crate::role::AuthRole;

/// Type alias for the authorizer closure used by ConnectionManager.
///
/// Takes a PeerIdentity and returns Some(AuthRole) if authorized, None if denied.
pub type Authorizer =
    Box<dyn Fn(&zznet_api::types::PeerIdentity) -> Option<AuthRole> + Send + Sync>;

/// Access Control List manager for authorization decisions.
///
/// This implements allow-list based authorization where only explicitly
/// allowed identities can access the system.
#[derive(Debug, Clone)]
pub struct AclManager {
    /// Set of allowed canonical peer strings (e.g. "alice@collector" or "collector").
    allowed_peers: HashSet<String>,
    /// Whether to trust HELLO messages in insecure (non-TLS) mode.
    insecure_trust_hello: bool,
}

impl AclManager {
    /// Creates a new ACL manager with no allowed users.
    pub fn new() -> Self {
        Self {
            allowed_peers: HashSet::new(),
            insecure_trust_hello: false,
        }
    }

    /// Creates an ACL manager from a set of allowed usernames.
    pub fn with_allowed_peers(allowed_peers: HashSet<String>) -> Self {
        Self {
            allowed_peers,
            insecure_trust_hello: false,
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
    /// Returns the resolved AuthRole on success.
    pub fn authorize_peer(
        &self,
        identity: &zznet_api::types::PeerIdentity,
    ) -> Result<AuthRole, crate::error::AuthError> {
        tracing::debug!(peer = %identity.full_identity(), "authorize_peer called");

        let canonical = format!("{}@{}", identity.san_username, identity.common_name);
        if self.allowed_peers.contains(&canonical) {
            tracing::info!(peer = %identity.full_identity(), "authorized by canonical match");
            return AuthRole::from_cn(&identity.common_name);
        }

        // Try role-only match
        if self.allowed_peers.contains(&identity.common_name) {
            tracing::info!(peer = %identity.full_identity(), role = %identity.common_name, "authorized by role-only match");
            return AuthRole::from_cn(&identity.common_name);
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
    /// This wraps `authorize_peer` to return `Option<AuthRole>` instead of `Result`,
    /// which matches the signature expected by ConnectionManager's authorizer closure.
    ///
    /// Returns `Some(role)` if authorized, `None` if denied or error.
    pub fn authorize_peer_option(
        &self,
        identity: &zznet_api::types::PeerIdentity,
    ) -> Option<AuthRole> {
        self.authorize_peer(identity).ok()
    }

    /// Create an Authorizer closure suitable for ConnectionManager.
    ///
    /// This is the recommended way to integrate AclManager with ConnectionManager.
    /// The returned closure captures the AclManager and can be passed to
    /// `ConnectionManager::new_with_acl()`.
    ///
    /// # Example
    /// ```ignore
    /// let acl = AclManager::with_allowed_peers(allowed_peers);
    /// let authorizer = acl.to_authorizer();
    /// let conn_mgr = ConnectionManager::new_with_acl(rooms, Some((authorizer, false)));
    /// ```
    pub fn to_authorizer(self) -> Authorizer {
        Box::new(move |identity| self.authorize_peer_option(identity))
    }
}

impl Default for AclManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_role_from_cn() {
        assert!(matches!(
            AuthRole::from_cn("collector"),
            Ok(AuthRole::Collector)
        ));
        assert!(matches!(
            AuthRole::from_cn("database"),
            Ok(AuthRole::Database)
        ));
        assert!(matches!(
            AuthRole::from_cn("client-ro"),
            Ok(AuthRole::ClientRo)
        ));
        assert!(matches!(
            AuthRole::from_cn("client-admin"),
            Ok(AuthRole::ClientAdmin)
        ));
        assert!(matches!(
            AuthRole::from_cn("invalid"),
            Err(crate::error::AuthError::UnknownRole(_))
        ));
    }

    #[test]
    fn test_role_conversion_roundtrip() {
        let api_role = zznet_api::types::Role::Collector;
        let auth_role = AuthRole::from_api_role(api_role);
        let converted_back = auth_role.to_api_role();
        assert_eq!(api_role, converted_back);
    }

    #[test]
    fn test_can_connect_to_collector_to_database() {
        assert!(AuthRole::Collector.can_connect_to(&AuthRole::Database));
        assert!(!AuthRole::Collector.can_connect_to(&AuthRole::Collector));
        assert!(!AuthRole::Collector.can_connect_to(&AuthRole::ClientRo));
    }

    #[test]
    fn test_can_connect_to_database_accepts() {
        assert!(AuthRole::Database.can_connect_to(&AuthRole::Collector));
        assert!(AuthRole::Database.can_connect_to(&AuthRole::ClientRo));
        assert!(AuthRole::Database.can_connect_to(&AuthRole::ClientAdmin));
        assert!(!AuthRole::Database.can_connect_to(&AuthRole::Database));
    }

    #[test]
    fn test_can_connect_to_admin_to_all() {
        assert!(AuthRole::ClientAdmin.can_connect_to(&AuthRole::Collector));
        assert!(AuthRole::ClientAdmin.can_connect_to(&AuthRole::Database));
        assert!(AuthRole::ClientAdmin.can_connect_to(&AuthRole::ClientRo));
        assert!(AuthRole::ClientAdmin.can_connect_to(&AuthRole::ClientAdmin));
    }

    #[test]
    fn test_can_access_room_collector() {
        assert!(AuthRole::Collector.can_access_room("memdb"));
        assert!(!AuthRole::Collector.can_access_room("query"));
        assert!(!AuthRole::Collector.can_access_room("unknown"));
    }

    #[test]
    fn test_can_access_room_database() {
        assert!(AuthRole::Database.can_access_room("memdb"));
        assert!(AuthRole::Database.can_access_room("query"));
        assert!(!AuthRole::Database.can_access_room("unknown"));
    }

    #[test]
    fn test_can_access_room_client_ro() {
        assert!(!AuthRole::ClientRo.can_access_room("memdb"));
        assert!(AuthRole::ClientRo.can_access_room("query"));
        assert!(!AuthRole::ClientRo.can_access_room("unknown"));
    }

    #[test]
    fn test_can_access_room_admin() {
        assert!(AuthRole::ClientAdmin.can_access_room("memdb"));
        assert!(AuthRole::ClientAdmin.can_access_room("query"));
        assert!(AuthRole::ClientAdmin.can_access_room("unknown"));
        assert!(AuthRole::ClientAdmin.can_access_room("anything"));
    }

    #[test]
    fn test_acl_manager_new() {
        let acl = AclManager::new();
        assert!(!acl.is_authorized("alice"));
        assert!(acl.allowed_peers().is_empty());
    }

    #[test]
    fn test_acl_manager_with_allowed_users() {
        let mut users = HashSet::new();
        users.insert("alice".to_string());
        users.insert("bob".to_string());
        let acl = AclManager::with_allowed_peers(users);
        assert!(acl.is_authorized("alice"));
        assert!(acl.is_authorized("bob"));
        assert!(!acl.is_authorized("charlie"));
    }

    #[test]
    fn test_acl_manager_allow_deny_user() {
        let mut acl = AclManager::new();
        assert!(!acl.is_authorized("alice"));

        acl.allow_user("alice");
        assert!(acl.is_authorized("alice"));

        acl.deny_user("alice");
        assert!(!acl.is_authorized("alice"));
    }

    #[test]
    fn test_authorize_peer_canonical_and_role() {
        let mut users = HashSet::new();
        users.insert("alice@collector".to_string());
        users.insert("database".to_string());

        let acl = AclManager::with_allowed_peers(users);

        let allowed_identity = zznet_api::types::PeerIdentity {
            common_name: "collector".to_string(),
            san_username: "alice".to_string(),
            peer_addr: "127.0.0.1:1234".to_string(),
        };

        // canonical match
        let role = acl.authorize_peer(&allowed_identity);
        assert!(matches!(role, Ok(AuthRole::Collector)));

        // role-only match
        let db_identity = zznet_api::types::PeerIdentity {
            common_name: "database".to_string(),
            san_username: "svc1".to_string(),
            peer_addr: "127.0.0.1:1235".to_string(),
        };

        let role2 = acl.authorize_peer(&db_identity);
        assert!(matches!(role2, Ok(AuthRole::Database)));

        // denied
        let denied = zznet_api::types::PeerIdentity {
            common_name: "collector".to_string(),
            san_username: "bob".to_string(),
            peer_addr: "127.0.0.1:1236".to_string(),
        };

        assert!(matches!(
            acl.authorize_peer(&denied),
            Err(crate::error::AuthError::IdentityNotAllowed(_))
        ));
    }

    #[test]
    fn test_auth_role_equality() {
        assert_eq!(AuthRole::Collector, AuthRole::Collector);
        assert_ne!(AuthRole::Collector, AuthRole::Database);
    }

    #[test]
    fn test_auth_role_copy() {
        let role = AuthRole::Collector;
        let copied = role;
        assert_eq!(role, copied);
    }

    #[test]
    fn test_acl_manager_integration() {
        // Test scenario: Create AclManager with allow-list, authorize allowed and denied peers
        let mut allowed = HashSet::new();
        allowed.insert("collector".to_string());
        allowed.insert("alice@client-admin".to_string());
        let acl = AclManager::with_allowed_peers_and_insecure(allowed, false);

        // Service identity: collector
        let service_identity = zznet_api::types::PeerIdentity {
            common_name: "collector".to_string(),
            san_username: "root".to_string(),
            peer_addr: "127.0.0.1:1234".to_string(),
        };
        assert!(matches!(
            acl.authorize_peer(&service_identity),
            Ok(AuthRole::Collector)
        ));

        // User identity: alice@client-admin
        let user_identity = zznet_api::types::PeerIdentity {
            common_name: "client-admin".to_string(),
            san_username: "alice".to_string(),
            peer_addr: "127.0.0.1:1235".to_string(),
        };
        assert!(matches!(
            acl.authorize_peer(&user_identity),
            Ok(AuthRole::ClientAdmin)
        ));

        // Denied peer
        let denied_identity = zznet_api::types::PeerIdentity {
            common_name: "client-ro".to_string(),
            san_username: "bob".to_string(),
            peer_addr: "127.0.0.1:1236".to_string(),
        };
        assert!(matches!(
            acl.authorize_peer(&denied_identity),
            Err(crate::error::AuthError::IdentityNotAllowed(_))
        ));

        // Insecure mode
        assert!(!acl.is_insecure_mode());
    }
}
