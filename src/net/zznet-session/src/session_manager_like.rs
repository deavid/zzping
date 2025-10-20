use crate::{
    room_message_trait::RoomMessageTrait,
    types::{PeerId, RoomId, SessionError},
};
use async_trait::async_trait;
use std::time::Duration;
use zznet_auth::ApplicationRole;

/// A trait for mocking the SessionManager.
///
/// This trait provides a subset of the `SessionManager`'s methods, allowing it
/// to be mocked for testing purposes.
#[async_trait]
pub trait SessionManagerLike<TMsg, TRole>: Send + Sync
where
    TMsg: RoomMessageTrait,
    TRole: ApplicationRole,
{
    /// Broadcast a message to all peers matching a filter in parallel.
    ///
    /// This helper sends `message` to the given `room_id` for every peer whose
    /// role satisfies `filter`. Each send is performed in its own task and the
    /// function returns a vector of per-peer results. A per-send timeout can be
    /// provided to avoid blocking on slow peers.
    async fn broadcast_to_room<F>(
        &self,
        room_id: &RoomId,
        message: TMsg,
        filter: F,
        timeout: Option<Duration>,
    ) -> Vec<(PeerId, Result<(), SessionError>)>
    where
        F: Fn(&TRole) -> bool + Send + Sync + 'static;

    /// Send a typed message to a specific peer's room
    ///
    /// The message is the application's enum type (TMsg).
    /// It will be serialized at the transport layer.
    async fn send_to_room(
        &self,
        peer_id: &PeerId,
        room_id: &RoomId,
        msg: TMsg,
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
impl<TMsg, TRole> SessionManagerLike<TMsg, TRole>
    for tokio::sync::Mutex<crate::session_manager::SessionManager<TMsg, TRole>>
where
    TMsg: RoomMessageTrait,
    TRole: ApplicationRole,
{
    async fn broadcast_to_room<F>(
        &self,
        room_id: &RoomId,
        message: TMsg,
        filter: F,
        timeout: Option<Duration>,
    ) -> Vec<(PeerId, Result<(), SessionError>)>
    where
        F: Fn(&TRole) -> bool + Send + Sync + 'static,
    {
        let timeout_duration = timeout.unwrap_or(Duration::from_millis(5000));
        // Lock the mutex to access the SessionManager
        let sm = self.lock().await;
        sm.broadcast_to_room(room_id, message, filter, timeout_duration)
            .await
    }

    async fn send_to_room(
        &self,
        peer_id: &PeerId,
        room_id: &RoomId,
        msg: TMsg,
    ) -> Result<(), SessionError> {
        let sm = self.lock().await;
        sm.send_to_room(peer_id, room_id, msg).await
    }

    fn get_peer_role(&self, peer_id: &PeerId) -> Option<TRole> {
        // This is a sync method, so we need to block on the async lock
        // This is OK for a quick operation like getting a role
        let sm = self.blocking_lock();
        sm.get_peer_role_cloned(peer_id)
    }
}
