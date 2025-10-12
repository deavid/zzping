//! Role configuration for MemDB component
//!
//! Defines the different roles a MemDB actor can take,
//! enabling the "same component, different config" pattern.

use std::path::PathBuf;

/// Role configuration for MemDB actor
///
/// This enum defines the two roles a MemDB actor can take:
/// - **Database**: Receives ping batches, stores data, provides query interface
/// - **Collector**: Buffers ping results, sends batches to database
///
/// This follows the Vision pattern: "Same component code on both sides,
/// just configured differently."
#[derive(Clone, Debug, PartialEq)]
pub enum MemDBRole {
    /// Database role: Receives batches, stores data, provides queries
    ///
    /// This role is typically used by the database process. It:
    /// - Receives ping result batches from Collector peers via network
    /// - Stores ping results in memory (or optionally to disk)
    /// - Provides query interface for AdminClient peers
    /// - Sends batch acknowledgments back to Collectors
    ///
    /// # Security
    /// - Accepts `SubmitBatch` from Collector role
    /// - Accepts `Query` from ClientAdmin role
    /// - Sends `BatchAck` to Collector role
    /// - Sends `QueryResponse` to ClientAdmin role
    ///
    /// # Behavior
    /// - Sends: BatchAck (to Collectors), QueryResponse (to AdminClients)
    /// - Receives: SubmitBatch (from Collectors), Query (from AdminClients)
    Database {
        /// Maximum number of results to store per target
        max_results_per_target: usize,
        /// Optional path for persistence (None = in-memory only)
        persistence_path: Option<PathBuf>,
    },

    /// Collector role: Buffers results, sends batches to database
    ///
    /// This role is typically used by the collector process. It:
    /// - Buffers ping results from the pinger component
    /// - Sends batches to Database peers via network when buffer is full
    /// - Receives batch acknowledgments from Database
    /// - Does NOT provide query interface
    ///
    /// # Security
    /// - Can send SubmitBatch (write-only to Database)
    /// - Can receive BatchAck (read-only from Database)
    /// - Cannot receive Query (no query access)
    ///
    /// # Behavior
    /// - Sends: SubmitBatch (to Database)
    /// - Receives: BatchAck (from Database), StorePingResult (from local pinger)
    Collector {
        /// Maximum number of results to buffer before sending batch
        buffer_size: usize,
    },
}

impl Default for MemDBRole {
    /// Default role is Collector (passive sender)
    ///
    /// This is safer than defaulting to Database which requires
    /// configuration parameters.
    fn default() -> Self {
        Self::Collector { buffer_size: 1000 }
    }
}

impl MemDBRole {
    /// Check if this is a Database role
    pub fn is_database(&self) -> bool {
        matches!(self, Self::Database { .. })
    }

    /// Check if this is a Collector role
    pub fn is_collector(&self) -> bool {
        matches!(self, Self::Collector { .. })
    }

    /// Returns the buffer size for Collector role, None for Database.
    ///
    /// Used for configuring the ping result buffer.
    pub fn buffer_size(&self) -> Option<usize> {
        match self {
            Self::Collector { buffer_size } => Some(*buffer_size),
            Self::Database { .. } => None,
        }
    }

    /// Returns the max results per target for Database role, None for Collector.
    ///
    /// Used for configuring storage limits.
    pub fn max_results_per_target(&self) -> Option<usize> {
        match self {
            Self::Database {
                max_results_per_target,
                ..
            } => Some(*max_results_per_target),
            Self::Collector { .. } => None,
        }
    }

    /// Returns the persistence path for Database role, None for Collector or in-memory.
    ///
    /// Used when persisting data to disk.
    pub fn persistence_path(&self) -> Option<&PathBuf> {
        match self {
            Self::Database {
                persistence_path, ..
            } => persistence_path.as_ref(),
            Self::Collector { .. } => None,
        }
    }

    /// Validate the role configuration
    ///
    /// Returns an error if the configuration is invalid.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Database {
                max_results_per_target,
                ..
            } => {
                if *max_results_per_target == 0 {
                    return Err("Database role requires max_results_per_target > 0".to_string());
                }
                Ok(())
            }
            Self::Collector { buffer_size } => {
                if *buffer_size == 0 {
                    return Err("Collector role requires buffer_size > 0".to_string());
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_creation() {
        let database = MemDBRole::Database {
            max_results_per_target: 10000,
            persistence_path: Some(PathBuf::from("/var/lib/zzping/memdb")),
        };
        let collector = MemDBRole::Collector { buffer_size: 500 };

        assert!(!database.is_collector());
        assert!(database.is_database());
        assert!(collector.is_collector());
        assert!(!collector.is_database());
    }

    #[test]
    fn test_default_role() {
        let default_role = MemDBRole::default();
        assert!(default_role.is_collector());
        assert_eq!(default_role.buffer_size(), Some(1000));
    }

    #[test]
    fn test_role_properties() {
        let database = MemDBRole::Database {
            max_results_per_target: 5000,
            persistence_path: Some(PathBuf::from("/tmp/memdb")),
        };
        let collector = MemDBRole::Collector { buffer_size: 200 };

        assert_eq!(database.max_results_per_target(), Some(5000));
        assert_eq!(
            database.persistence_path(),
            Some(&PathBuf::from("/tmp/memdb"))
        );
        assert_eq!(database.buffer_size(), None);

        assert_eq!(collector.buffer_size(), Some(200));
        assert_eq!(collector.max_results_per_target(), None);
        assert_eq!(collector.persistence_path(), None);
    }

    #[test]
    fn test_validation_success() {
        let database = MemDBRole::Database {
            max_results_per_target: 1000,
            persistence_path: None,
        };
        let collector = MemDBRole::Collector { buffer_size: 100 };

        assert!(database.validate().is_ok());
        assert!(collector.validate().is_ok());
    }

    #[test]
    fn test_validation_zero_values() {
        let database = MemDBRole::Database {
            max_results_per_target: 0,
            persistence_path: None,
        };
        let collector = MemDBRole::Collector { buffer_size: 0 };

        assert!(database.validate().is_err());
        assert!(collector.validate().is_err());
    }

    #[test]
    fn test_role_equality() {
        let database1 = MemDBRole::Database {
            max_results_per_target: 1000,
            persistence_path: Some(PathBuf::from("/tmp/memdb")),
        };
        let database2 = MemDBRole::Database {
            max_results_per_target: 1000,
            persistence_path: Some(PathBuf::from("/tmp/memdb")),
        };
        let database3 = MemDBRole::Database {
            max_results_per_target: 2000,
            persistence_path: None,
        };
        let collector = MemDBRole::Collector { buffer_size: 100 };

        assert_eq!(database1, database2);
        assert_ne!(database1, database3);
        assert_ne!(database1, collector);
    }
}
