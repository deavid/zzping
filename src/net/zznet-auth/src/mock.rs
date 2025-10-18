//! Mock roles for testing zznet-auth functionality.
//!
//! This module provides simple mock role implementations that can be used
//! in tests without depending on application-specific roles.

use super::{ApplicationRole, error::AuthError};
use serde::{Deserialize, Serialize};

/// Simple mock role enum for testing purposes.
///
/// This provides a minimal role implementation with basic authorization rules
/// suitable for unit and integration tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MockRole {
    /// Administrative role with full access.
    Admin,
    /// Regular user role with limited access.
    User,
    /// Guest role with minimal access.
    Guest,
}

impl ApplicationRole for MockRole {
    fn from_cn(cn: &str) -> Result<Self, AuthError> {
        match cn {
            "admin" => Ok(MockRole::Admin),
            "user" => Ok(MockRole::User),
            "guest" => Ok(MockRole::Guest),
            _ => Err(AuthError::UnknownRole(cn.to_string())),
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            MockRole::Admin => "admin",
            MockRole::User => "user",
            MockRole::Guest => "guest",
        }
    }

    fn can_connect_to(&self, _target: &Self) -> bool {
        // Permissive for tests - all roles can connect to all other roles
        true
    }

    fn can_access_room(&self, room_name: &str) -> bool {
        match self {
            MockRole::Admin => true, // Admin can access all rooms
            MockRole::User => room_name == "user-room" || room_name == "shared-room",
            MockRole::Guest => room_name == "shared-room",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_role_from_cn() {
        assert!(matches!(MockRole::from_cn("admin"), Ok(MockRole::Admin)));
        assert!(matches!(MockRole::from_cn("user"), Ok(MockRole::User)));
        assert!(matches!(MockRole::from_cn("guest"), Ok(MockRole::Guest)));
        assert!(matches!(
            MockRole::from_cn("invalid"),
            Err(AuthError::UnknownRole(_))
        ));
    }

    #[test]
    fn test_mock_role_as_str() {
        assert_eq!(MockRole::Admin.as_str(), "admin");
        assert_eq!(MockRole::User.as_str(), "user");
        assert_eq!(MockRole::Guest.as_str(), "guest");
    }

    #[test]
    fn test_mock_role_can_connect_to() {
        // All combinations should be true for mock roles
        assert!(MockRole::Admin.can_connect_to(&MockRole::Admin));
        assert!(MockRole::Admin.can_connect_to(&MockRole::User));
        assert!(MockRole::User.can_connect_to(&MockRole::Guest));
    }

    #[test]
    fn test_mock_role_can_access_room() {
        // Admin can access everything
        assert!(MockRole::Admin.can_access_room("user-room"));
        assert!(MockRole::Admin.can_access_room("shared-room"));
        assert!(MockRole::Admin.can_access_room("admin-room"));

        // User can access user-room and shared-room
        assert!(MockRole::User.can_access_room("user-room"));
        assert!(MockRole::User.can_access_room("shared-room"));
        assert!(!MockRole::User.can_access_room("admin-room"));

        // Guest can only access shared-room
        assert!(MockRole::Guest.can_access_room("shared-room"));
        assert!(!MockRole::Guest.can_access_room("user-room"));
        assert!(!MockRole::Guest.can_access_room("admin-room"));
    }
}
