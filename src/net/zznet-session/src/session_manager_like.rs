use crate::types::{PeerId, RoomId, SessionError};
use async_trait::async_trait;
use zznet_auth::ApplicationRole;

/// A trait for mocking the SessionManager.
///
/// This trait provides a subset of the `SessionManager`'s methods, allowing it
/// to be mocked for testing purposes.
#[async_trait]
pub trait SessionManagerLike<TRole>: Send + Sync
where
    TRole: ApplicationRole,
{
    /// Send serialized bytes to a specific peer's room
    ///
    /// Serialization happens at the Room layer.
    async fn send_to_room(
        &self,
        peer_id: &PeerId,
        room_id: &RoomId,
        bytes: Vec<u8>,
    ) -> Result<(), SessionError>;

    /// Get the authenticated role for a peer, cloned out of the session manager.
    ///
    /// Implementers should return `None` when role information is not available
    /// (for example, ACL not configured or role resolution failed).
    fn get_peer_role(&self, peer_id: &PeerId) -> Option<TRole>;
}

// Implementation of SessionManagerLike for tokio::sync::Mutex<SessionManager>
// This allows components to use Arc<Mutex<SessionManager>> as their session manager,
// enabling sharing with ConnectionManager which needs mutable access.
#[async_trait]
impl<TRole> SessionManagerLike<TRole>
    for tokio::sync::Mutex<crate::session_manager::SessionManager<TRole>>
where
    TRole: ApplicationRole,
{
    async fn send_to_room(
        &self,
        peer_id: &PeerId,
        room_id: &RoomId,
        bytes: Vec<u8>,
    ) -> Result<(), SessionError> {
        let sm = self.lock().await;
        sm.send_to_room(peer_id, room_id, bytes).await
    }

    fn get_peer_role(&self, peer_id: &PeerId) -> Option<TRole> {
        // This is a sync method, so we need to block on the async lock
        // This is OK for a quick operation like getting a role
        let sm = self.blocking_lock();
        sm.get_peer_role_cloned(peer_id)
    }
}
