//! Role configuration for IntentConfig component
//!
//! Defines the different roles an IntentConfig actor can take,
//! enabling the "same component, different config" pattern.

use std::path::PathBuf;

/// Role configuration for IntentConfig actor
///
/// This enum defines the two roles an IntentConfig actor can take:
/// - **Collector**: Has a file watcher, reads config from disk, broadcasts updates
/// - **Database**: Passive receiver, relays updates to local subscribers
///
/// This follows the Vision pattern: "Same component code on both sides,
/// just configured differently."
///
/// # Example
///
/// ```ignore
/// // Collector process
/// let collector = IntentConfigBuilder::new()
///     .role(IntentConfigRole::Collector {
///         config_file_path: PathBuf::from("/etc/zzping/intent.ron"),
///     })
///     .start();
///
/// // Database process
/// let database = IntentConfigBuilder::new()
///     .role(IntentConfigRole::Database)
///     .start();
/// ```
#[derive(Clone, Debug, PartialEq)]
pub enum IntentConfigRole {
    /// Collector role: Has file watcher, broadcasts config updates
    ///
    /// This role is typically used by the collector process. It:
    /// - Watches a configuration file for changes
    /// - Loads configuration from disk
    /// - Broadcasts updates to Database peers via network
    /// - Responds to QueryCurrentConfig requests from Database
    ///
    /// # Behavior
    /// - Sends: ConfigUpdate, CurrentConfig
    /// - Receives: QueryCurrentConfig
    Collector {
        /// Path to the configuration file to watch
        config_file_path: PathBuf,
    },

    /// Database role: Passive receiver, relays to subscribers
    ///
    /// This role is typically used by the database process. It:
    /// - Receives configuration updates from Collector via network
    /// - Stores current configuration state
    /// - Broadcasts to local subscribers (e.g., MemDB)
    /// - Can query Collector for current config on startup
    ///
    /// # Behavior
    /// - Sends: QueryCurrentConfig (optional, on startup)
    /// - Receives: ConfigUpdate, CurrentConfig
    Database,
}

impl Default for IntentConfigRole {
    /// Default role is Database (passive receiver)
    ///
    /// This is safer than defaulting to Collector which requires
    /// a file path that might not exist.
    fn default() -> Self {
        Self::Database
    }
}

impl IntentConfigRole {
    /// Check if this is a Collector role
    pub fn is_collector(&self) -> bool {
        matches!(self, Self::Collector { .. })
    }

    /// Check if this is a Database role
    pub fn is_database(&self) -> bool {
        matches!(self, Self::Database)
    }

    /// Get the config file path if this is a Collector role
    pub fn config_file_path(&self) -> Option<&PathBuf> {
        match self {
            Self::Collector { config_file_path } => Some(config_file_path),
            Self::Database => None,
        }
    }

    /// Validate the role configuration
    ///
    /// Returns an error if the configuration is invalid.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Collector { config_file_path } => {
                if config_file_path.as_os_str().is_empty() {
                    return Err("Collector role requires non-empty config_file_path".to_string());
                }
                Ok(())
            }
            Self::Database => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_creation() {
        let collector = IntentConfigRole::Collector {
            config_file_path: PathBuf::from("/etc/intent.ron"),
        };
        let database = IntentConfigRole::Database;

        assert!(collector.is_collector());
        assert!(!collector.is_database());
        assert!(!database.is_collector());
        assert!(database.is_database());
    }

    #[test]
    fn test_default_role() {
        let default_role = IntentConfigRole::default();
        assert!(default_role.is_database());
    }

    #[test]
    fn test_config_file_path() {
        let path = PathBuf::from("/etc/intent.ron");
        let collector = IntentConfigRole::Collector {
            config_file_path: path.clone(),
        };
        let database = IntentConfigRole::Database;

        assert_eq!(collector.config_file_path(), Some(&path));
        assert_eq!(database.config_file_path(), None);
    }

    #[test]
    fn test_validation_success() {
        let collector = IntentConfigRole::Collector {
            config_file_path: PathBuf::from("/etc/intent.ron"),
        };
        let database = IntentConfigRole::Database;

        assert!(collector.validate().is_ok());
        assert!(database.validate().is_ok());
    }

    #[test]
    fn test_validation_empty_path() {
        let collector = IntentConfigRole::Collector {
            config_file_path: PathBuf::from(""),
        };

        let result = collector.validate();
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .contains("requires non-empty config_file_path")
        );
    }

    #[test]
    fn test_role_equality() {
        let collector1 = IntentConfigRole::Collector {
            config_file_path: PathBuf::from("/etc/intent.ron"),
        };
        let collector2 = IntentConfigRole::Collector {
            config_file_path: PathBuf::from("/etc/intent.ron"),
        };
        let collector3 = IntentConfigRole::Collector {
            config_file_path: PathBuf::from("/tmp/intent.ron"),
        };
        let database = IntentConfigRole::Database;

        assert_eq!(collector1, collector2);
        assert_ne!(collector1, collector3);
        assert_ne!(collector1, database);
    }
}
