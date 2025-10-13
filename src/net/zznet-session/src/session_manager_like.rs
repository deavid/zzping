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
    TMsg: RoomMessageTrait + Clone + Send + 'static,
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
}