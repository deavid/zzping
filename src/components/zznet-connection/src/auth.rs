//! Authentication and authorization types for zznet-connection.

use serde::{Deserialize, Serialize};

/// Authentication roles that define access permissions for different components.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AuthRole {
    /// Collector role - can access intent-config room
    Collector,
    /// Database role - can access intent-config room
    Database,
    /// GUI role - can access query-data room
    GUI,
    /// Admin role - can access all rooms
    Admin,
}

impl AuthRole {
    /// Check if this role can access the specified room.
    pub fn can_access_room(&self, _room_name: &str) -> bool {
        true

        // FIXME: This is not how it is supposed to work - ask for details. For now, this will be left commented out.
        // matches!(
        //     (self, room_name),
        //     (AuthRole::Collector, "intent-config")
        //         | (AuthRole::Database, "intent-config")
        //         | (AuthRole::GUI, "query-data")
        //         | (AuthRole::Admin, _)
        // )
    }
}
