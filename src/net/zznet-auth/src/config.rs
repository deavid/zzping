//! ACL configuration loading and validation.
//!
//! This module provides TOML-based configuration for access control lists,
//! allowing administrators to define which peers are allowed to access the system.

use crate::error::AuthError;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

/// ACL configuration loaded from TOML file.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AclConfig {
    /// Canonical peer strings allowed, e.g. "alice@collector" or just "collector".
    #[serde(default)]
    pub allowed_peers: Vec<String>,

    /// Whether to allow insecure trust after HELLO (for plain TCP debugging).
    #[serde(default)]
    pub insecure_trust_hello: bool,
}

impl AclConfig {
    /// Loads ACL configuration from a TOML file.
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, AuthError> {
        let content = std::fs::read_to_string(path)?;
        let config: AclConfig = toml::from_str(&content)?;
        Ok(config)
    }

    /// Creates an ACL configuration from a list of allowed peers.
    pub fn new(allowed_peers: Vec<String>, insecure_trust_hello: bool) -> Self {
        Self {
            allowed_peers,
            insecure_trust_hello,
        }
    }

    /// Validates the configuration.
    pub fn validate(&self) -> Result<(), AuthError> {
        let mut seen = HashSet::new();

        for peer in &self.allowed_peers {
            if peer.trim().is_empty() {
                return Err(AuthError::ConfigError(
                    "Empty entry in allowed_peers".to_string(),
                ));
            }

            if !seen.insert(peer.clone()) {
                return Err(AuthError::ConfigError(format!(
                    "Duplicate entry in allowed_peers: {}",
                    peer
                )));
            }
        }

        // Additional validation/warnings
        // Warn if role-only and user@role form both exist for same role
        for peer in &self.allowed_peers {
            if !peer.contains('@') {
                let role = peer.as_str();
                if self
                    .allowed_peers
                    .iter()
                    .any(|p| p.ends_with(&format!("@{}", role)))
                {
                    tracing::warn!(
                        "ACL contains both role-only '{}' and user@role entries for the same role",
                        role
                    );
                }
            }
        }

        // Note: Role validation is application-specific and should be done
        // by the application that implements the ApplicationRole trait

        // Warn if the ACL is overly permissive (role-only entries exist)
        if self.allowed_peers.iter().any(|p| !p.contains('@')) {
            tracing::warn!(
                "ACL contains role-only entries which allow any user with that role; consider using user@role entries for better security"
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_acl_config_new_and_validate() {
        let cfg = AclConfig::new(
            vec!["alice@collector".to_string(), "database".to_string()],
            false,
        );
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn test_acl_config_empty_entry() {
        let cfg = AclConfig::new(vec!["alice".to_string(), "".to_string()], false);
        assert!(matches!(cfg.validate(), Err(AuthError::ConfigError(_))));
    }

    #[test]
    fn test_acl_config_from_file() {
        let mut tf = NamedTempFile::new().unwrap();
        let toml = r#"
            allowed_peers = ["alice@collector", "database"]
            insecure_trust_hello = true
        "#;
        write!(tf, "{}", toml).unwrap();

        let cfg = AclConfig::from_file(tf.path()).unwrap();
        assert_eq!(cfg.allowed_peers.len(), 2);
        assert!(cfg.insecure_trust_hello);
    }
}
