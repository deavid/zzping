//! Role configuration for IntentConfig component
//!
//! Defines the different roles an IntentConfig actor can take,
//! enabling the "same component, different config" pattern.

use std::path::PathBuf;

/// Role configuration for IntentConfig actor
///
/// This enum defines the two roles an IntentConfig actor can take:
/// - **Database**: Authoritative source, sends config updates
/// - **Collector**: Passive receiver, accepts config updates
///
/// This follows the Vision pattern: "Same component code on both sides,
/// just configured differently."
#[derive(Clone, Debug, PartialEq)]
pub enum IntentConfigRole {
    /// Database role: Authoritative source, sends config updates
    ///
    /// This role is typically used by the database process. It:
    /// - Stores current configuration state
    /// - Persists configuration to disk
    /// - Receives config change requests from AdminClient
    /// - Sends ConfigUpdate individually to each Collector peer via network (1:1 rooms)
    ///
    /// # Behavior
    /// - Sends: ConfigUpdate
    /// - Receives: RequestConfigChange (from AdminClient)
    Database {
        /// Path to the configuration file to persist to
        config_file_path: PathBuf,
    },

    /// Collector role: Passive subscriber, receives config updates
    ///
    /// This role is typically used by the collector process. It:
    /// - Receives configuration updates from Database via network
    /// - Applies received configuration to local operations
    /// - Does NOT read from disk or watch files
    /// - Does NOT send configuration updates
    ///
    /// # Behavior
    /// - Sends: Nothing (purely receives)
    /// - Receives: ConfigUpdate
    Collector,
}

impl Default for IntentConfigRole {
    /// Default role is Collector (passive receiver)
    ///
    /// This is safer than defaulting to Database which requires
    /// a file path that might not exist.
    fn default() -> Self {
        Self::Collector
    }
}

impl IntentConfigRole {
    /// Check if this is a Database role
    pub fn is_database(&self) -> bool {
        matches!(self, Self::Database { .. })
    }

    /// Check if this is a Collector role
    pub fn is_collector(&self) -> bool {
        matches!(self, Self::Collector)
    }

    /// Returns the persistence file path for Database role, None for Collector.
    ///
    /// Used when loading initial state on startup or verifying configuration
    /// before starting the actor.
    pub fn config_file_path(&self) -> Option<&PathBuf> {
        match self {
            Self::Database { config_file_path } => Some(config_file_path),
            Self::Collector => None,
        }
    }

    /// Validate the role configuration
    ///
    /// Returns an error if the configuration is invalid.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Database { config_file_path } => {
                if config_file_path.as_os_str().is_empty() {
                    return Err("Database role requires non-empty config_file_path".to_string());
                }
                Ok(())
            }
            Self::Collector => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_creation() {
        let database = IntentConfigRole::Database {
            config_file_path: PathBuf::from("/etc/intent.ron"),
        };
        let collector = IntentConfigRole::Collector;

        assert!(!database.is_collector());
        assert!(database.is_database());
        assert!(collector.is_collector());
        assert!(!collector.is_database());
    }

    #[test]
    fn test_default_role() {
        let default_role = IntentConfigRole::default();
        assert!(default_role.is_collector());
    }

    #[test]
    fn test_config_file_path() {
        let path = PathBuf::from("/etc/intent.ron");
        let database = IntentConfigRole::Database {
            config_file_path: path.clone(),
        };
        let collector = IntentConfigRole::Collector;

        assert_eq!(database.config_file_path(), Some(&path));
        assert_eq!(collector.config_file_path(), None);
    }

    #[test]
    fn test_validation_success() {
        let database = IntentConfigRole::Database {
            config_file_path: PathBuf::from("/etc/intent.ron"),
        };
        let collector = IntentConfigRole::Collector;

        assert!(database.validate().is_ok());
        assert!(collector.validate().is_ok());
    }

    #[test]
    fn test_validation_empty_path() {
        let database = IntentConfigRole::Database {
            config_file_path: PathBuf::from(""),
        };

        let result = database.validate();
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .contains("requires non-empty config_file_path")
        );
    }

    #[test]
    fn test_role_equality() {
        let database1 = IntentConfigRole::Database {
            config_file_path: PathBuf::from("/etc/intent.ron"),
        };
        let database2 = IntentConfigRole::Database {
            config_file_path: PathBuf::from("/etc/intent.ron"),
        };
        let database3 = IntentConfigRole::Database {
            config_file_path: PathBuf::from("/tmp/intent.ron"),
        };
        let collector = IntentConfigRole::Collector;

        assert_eq!(database1, database2);
        assert_ne!(database1, database3);
        assert_ne!(database1, collector);
    }
}
