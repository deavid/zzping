//! Defines the operational roles for the `zzcollector-state` component.

/// Specifies the role of a `CStateActor` instance.
#[derive(Debug, Clone)]
pub enum CStateRole {
    /// The Collector role is responsible for reporting its health status
    /// to a central database.
    Collector {
        /// A unique identifier for this collector.
        collector_id: String,
        /// The interval in seconds at which to send heartbeats.
        heartbeat_interval_secs: u64,
    },
    /// The Database role tracks the status and health of multiple collectors.
    Database {
        /// The number of seconds without a heartbeat before a collector is considered stale.
        stale_timeout_secs: u64,
        /// The maximum number of collectors to track.
        max_collectors: Option<usize>,
    },
    /// The Admin role can query the database for information about collectors.
    Admin,
}

impl CStateRole {
    /// Returns `true` if the role is `Collector`.
    pub fn is_collector(&self) -> bool {
        matches!(self, Self::Collector { .. })
    }

    /// Returns `true` if the role is `Database`.
    pub fn is_database(&self) -> bool {
        matches!(self, Self::Database { .. })
    }

    /// Returns `true` if the role is `Admin`.
    pub fn is_admin(&self) -> bool {
        matches!(self, Self::Admin)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verifies the behavior of the `is_collector` method.
    #[test]
    fn test_is_collector() {
        let collector_role = CStateRole::Collector {
            collector_id: "test".to_string(),
            heartbeat_interval_secs: 5,
        };
        let database_role = CStateRole::Database {
            stale_timeout_secs: 15,
            max_collectors: None,
        };
        let admin_role = CStateRole::Admin;

        assert!(collector_role.is_collector());
        assert!(!database_role.is_collector());
        assert!(!admin_role.is_collector());
    }

    /// Verifies the behavior of the `is_database` method.
    #[test]
    fn test_is_database() {
        let collector_role = CStateRole::Collector {
            collector_id: "test".to_string(),
            heartbeat_interval_secs: 5,
        };
        let database_role = CStateRole::Database {
            stale_timeout_secs: 15,
            max_collectors: None,
        };
        let admin_role = CStateRole::Admin;

        assert!(!collector_role.is_database());
        assert!(database_role.is_database());
        assert!(!admin_role.is_database());
    }

    /// Verifies the behavior of the `is_admin` method.
    #[test]
    fn test_is_admin() {
        let collector_role = CStateRole::Collector {
            collector_id: "test".to_string(),
            heartbeat_interval_secs: 5,
        };
        let database_role = CStateRole::Database {
            stale_timeout_secs: 15,
            max_collectors: None,
        };
        let admin_role = CStateRole::Admin;

        assert!(!collector_role.is_admin());
        assert!(!database_role.is_admin());
        assert!(admin_role.is_admin());
    }
}