//! Generic room handler registry for simplified component wiring.
//!
//! This module provides utilities to register room handlers with a SessionManager,
//! eliminating the need for each application to duplicate this logic.

use std::sync::Arc;
use tokio::sync::Mutex;
use zznet_auth::ApplicationRole;
use zznet_session::peer_session::RoomHandle;
use zznet_session::room_message_trait::RoomMessageTrait;
use zznet_session::session_manager::SessionManager;
use zznet_session::types::RoomId;

/// A simple factory trait for creating room handlers.
///
/// Applications implement this to define how a specific room type
/// is handled for a given message type.
pub trait RoomHandlerFactory<TMsg, TRole>: Send + Sync
where
    TMsg: RoomMessageTrait + Send + Sync + 'static,
    TRole: ApplicationRole,
{
    /// Create a new room handler for the given room ID.
    fn create_handler(&self, room_id: RoomId) -> Box<dyn RoomHandle>;
}

/// Registry that manages room handler registration with a SessionManager.
///
/// This provides a cleaner API for apps to:
/// 1. Define room handlers once
/// 2. Register them with all peers (startup)
/// 3. Register them with new peers (dynamic)
pub struct RoomRegistry<TMsg, TRole>
where
    TMsg: RoomMessageTrait + Send + Sync + 'static,
    TRole: ApplicationRole,
{
    session_manager: Arc<Mutex<SessionManager<TRole>>>,
    // Map of room_id -> factory for creating handlers
    handlers: std::collections::HashMap<RoomId, Arc<dyn RoomHandlerFactory<TMsg, TRole>>>,
}

impl<TMsg, TRole> RoomRegistry<TMsg, TRole>
where
    TMsg: RoomMessageTrait + Send + Sync + 'static,
    TRole: ApplicationRole,
{
    /// Create a new room registry.
    pub fn new(session_manager: Arc<Mutex<SessionManager<TRole>>>) -> Self {
        Self {
            session_manager,
            handlers: std::collections::HashMap::new(),
        }
    }

    /// Register a handler factory for a specific room.
    ///
    /// # Arguments
    /// - `room_id`: The room to handle
    /// - `factory`: Factory for creating room handlers
    pub fn register_room_handler(
        &mut self,
        room_id: RoomId,
        factory: Arc<dyn RoomHandlerFactory<TMsg, TRole>>,
    ) {
        self.handlers.insert(room_id, factory);
    }

    /// Wire all registered room handlers with all existing peers.
    ///
    /// Call this during service startup after creating the SessionManager.
    pub async fn wire_all_peers(&self) -> Result<(), String> {
        let mut sm = self.session_manager.lock().await;

        let peer_ids: Vec<_> = sm.peer_ids();
        for peer_id in peer_ids {
            for (room_id, factory) in &self.handlers {
                let handler = factory.create_handler(room_id.clone());
                sm.add_room_to_peer(&peer_id, room_id.clone(), handler)
                    .await
                    .map_err(|e| {
                        format!("Failed to add room {} to peer {}: {}", room_id, peer_id, e)
                    })?;
            }
        }

        Ok(())
    }

    /// Wire room handlers for a newly-connected peer.
    ///
    /// Call this when a new peer connects dynamically.
    pub async fn wire_peer(&self, peer_id: &zznet_session::types::PeerId) -> Result<(), String> {
        let mut sm = self.session_manager.lock().await;

        for (room_id, factory) in &self.handlers {
            let handler = factory.create_handler(room_id.clone());
            sm.add_room_to_peer(peer_id, room_id.clone(), handler)
                .await
                .map_err(|e| {
                    format!("Failed to add room {} to peer {}: {}", room_id, peer_id, e)
                })?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_room_registry_creation() {
        // Verify registry can be created with a SessionManager
        // Full tests would require concrete message and role types from application
    }
}
