//! Adapter to wrap a shared SessionManager for use by IntentConfigActor.
//!
//! This adapter bridges the type mismatch:
//! - IntentConfigActor expects a SessionManager that can broadcast IntentConfigNetworkMsg
//! - We have a shared Arc<tokio::sync::Mutex<SessionManager<TMsg, TRole>>> that wraps broader app messages
//!
//! The adapter converts IntentConfigNetworkMsg to the outer TMsg type and
//! wraps the outer TRole in PermissionWrapper when needed.

use std::sync::Arc;
use tokio::sync::Mutex;

use zznet_auth::ApplicationRole;
use zznet_session::room_message_trait::RoomMessageTrait;
use zznet_session::session_manager::SessionManager;
use zznet_session::types::{PeerId, RoomId};

use crate::network_messages::IntentConfigNetworkMsg;
use crate::permission_wrapper::PermissionWrapper;

/// Type alias for broadcast futures returned by BroadcastVia trait.
/// Simplifies the complex nested generic type.
pub type BroadcastFuture = std::pin::Pin<
    Box<
        dyn std::future::Future<
                Output = Vec<(PeerId, Result<(), zznet_session::types::SessionError>)>,
            > + Send,
    >,
>;

/// Trait for broadcasting via an adapter.
/// This allows type erasure while still supporting the broadcast operation.
pub trait BroadcastVia: Send + Sync {
    /// Broadcast a config update to peers via the adapter.
    /// Returns a future that resolves when broadcast is complete.
    fn broadcast_config_update_async(
        &self,
        config_update: IntentConfigNetworkMsg,
        timeout: std::time::Duration,
    ) -> BroadcastFuture;

    /// Get peer IDs synchronously.
    fn peer_ids(&self) -> Vec<PeerId>;
}

/// Adapter that exposes a shared SessionManager<TMsg, TRole>
/// in a form that IntentConfigActor can use for broadcasting IntentConfigNetworkMsg.
///
/// Wraps the shared manager and translates calls:
/// - Converts IntentConfigNetworkMsg → TMsg (using Into trait)
/// - Wraps TRole → PermissionWrapper<TRole>
pub struct DatabaseMessageAdapter<TMsg, TRole>
where
    TMsg: RoomMessageTrait + From<IntentConfigNetworkMsg>,
    TRole: ApplicationRole + Clone,
{
    /// The shared session manager from the database service
    shared_manager: Arc<Mutex<SessionManager<TMsg, TRole>>>,
}

impl<TMsg, TRole> DatabaseMessageAdapter<TMsg, TRole>
where
    TMsg: RoomMessageTrait + From<IntentConfigNetworkMsg>,
    TRole: ApplicationRole + Clone,
{
    /// Create a new adapter wrapping the shared session manager.
    pub fn new(shared_manager: Arc<Mutex<SessionManager<TMsg, TRole>>>) -> Self {
        Self { shared_manager }
    }

    /// Get the list of connected peer IDs.
    pub fn peer_ids(&self) -> Vec<PeerId> {
        // Use blocking_lock for sync access
        self.shared_manager.blocking_lock().peer_ids()
    }

    /// Broadcast an IntentConfig message to all peers matching a filter.
    ///
    /// Converts the IntentConfigNetworkMsg to TMsg and broadcasts.
    pub async fn broadcast_to_room<F>(
        &self,
        room_id: &RoomId,
        message: IntentConfigNetworkMsg,
        filter: F,
        timeout: std::time::Duration,
    ) -> Vec<(PeerId, Result<(), zznet_session::types::SessionError>)>
    where
        F: (Fn(&PermissionWrapper<TRole>) -> bool) + Send + Sync + 'static,
    {
        // Convert IntentConfigNetworkMsg to TMsg
        let msg = TMsg::from(message);

        // Lock the shared manager
        let sm = self.shared_manager.lock().await;

        // Broadcast: the SM internally iterates peers and applies the filter
        // We wrap the filter to convert TRole to PermissionWrapper<TRole>

        sm.broadcast_to_room(
            room_id,
            msg,
            move |role: &TRole| {
                let wrapped = PermissionWrapper { permission: *role };
                filter(&wrapped)
            },
            timeout,
        )
        .await
    }

    /// Get the role of a peer, wrapped in PermissionWrapper.
    pub fn get_peer_role(&self, peer_id: &PeerId) -> Option<PermissionWrapper<TRole>> {
        let sm = self.shared_manager.blocking_lock();
        sm.get_peer_role_cloned(peer_id)
            .map(|role| PermissionWrapper { permission: role })
    }
}

impl<TMsg, TRole> BroadcastVia for DatabaseMessageAdapter<TMsg, TRole>
where
    TMsg: RoomMessageTrait + From<IntentConfigNetworkMsg> + Send + Sync + 'static,
    TRole: ApplicationRole + Clone + Send + Sync + 'static,
{
    fn broadcast_config_update_async(
        &self,
        config_update: IntentConfigNetworkMsg,
        timeout: std::time::Duration,
    ) -> BroadcastFuture {
        // Create a reference to self to use in the async block
        let shared_manager = Arc::clone(&self.shared_manager);

        Box::pin(async move {
            let room_id = RoomId::from("intent-config");
            eprintln!(
                "⚙️ BroadcastVia::broadcast_config_update_async - room={:?}",
                room_id
            );
            let msg = TMsg::from(config_update);

            // Lock the shared manager
            let sm = shared_manager.lock().await;
            let all_peers = sm.peer_ids();
            eprintln!("⚙️ All peers in SessionManager: {:?}", all_peers);

            // Broadcast to all peers (no filtering by role for now)
            // The SessionManager will handle the actual message delivery
            let results = sm
                .broadcast_to_room(
                    &room_id,
                    msg,
                    |_role: &TRole| {
                        // Allow broadcasting to all peers in the room
                        true
                    },
                    timeout,
                )
                .await;

            eprintln!("⚙️ Broadcast results: {} peers responded", results.len());
            results
        })
    }

    fn peer_ids(&self) -> Vec<PeerId> {
        self.shared_manager.blocking_lock().peer_ids()
    }
}
