//! RouterActor - Actix wrapper for Router
//!
//! Provides actor-based interface for Router, enabling async messaging
//! from components and PeerManager.

use crate::router::Router;
use actix::prelude::*;
use std::sync::Arc;
use zznet_api::messages::OnPeerConnected;
use zznet_api::types::{PeerId, RoomId};
use zznet_room::room_manager::RoomManager;

/// RouterActor - Actix wrapper for Router
///
/// Provides message-based async access to Router for components and PeerManager.
pub struct RouterActor {
    router: Arc<tokio::sync::Mutex<Router>>,
}

impl RouterActor {
    /// Create a new RouterActor
    pub fn new(offered_rooms: Vec<RoomId>) -> Self {
        Self {
            router: Arc::new(tokio::sync::Mutex::new(Router::new(offered_rooms))),
        }
    }
}

impl Actor for RouterActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        tracing::info!("RouterActor started");
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        tracing::info!("RouterActor stopped");
    }
}

// ============================================================================
// Messages
// ============================================================================

/// Register a room manager with the router
#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct RegisterManager {
    /// The room manager to register
    pub manager: Arc<dyn RoomManager + Send + Sync>,
}

impl Handler<RegisterManager> for RouterActor {
    type Result = ResponseFuture<Result<(), String>>;

    fn handle(&mut self, msg: RegisterManager, _ctx: &mut Context<Self>) -> Self::Result {
        let router_arc = self.router.clone();
        let manager = msg.manager;

        Box::pin(async move {
            let mut router = router_arc.lock().await;
            router
                .register_manager(manager)
                .map_err(|e| format!("Failed to register manager: {:?}", e))
        })
    }
}

impl Handler<OnPeerConnected> for RouterActor {
    type Result = ResponseFuture<Result<(), String>>;

    fn handle(&mut self, msg: OnPeerConnected, _ctx: &mut Context<Self>) -> Self::Result {
        let router_arc = self.router.clone();
        let peer_id = msg.peer_id.clone();
        let role = msg.role;
        let negotiated_rooms = msg.negotiated_rooms;
        let outbound_tx = msg.outbound_tx;
        let inbound_rx = msg.inbound_rx;

        Box::pin(async move {
            // Gather rooms from managers, but ONLY for negotiated rooms
            let mut builder = crate::peer_channels::PeerChannelsBuilder::new(peer_id.clone());
            {
                let router = router_arc.lock().await;
                // Iterate over the SUCCESSFULLY negotiated rooms
                for room_id in negotiated_rooms {
                    // Find the manager responsible for this room
                    if let Some(manager) = router
                        .managers
                        .values()
                        .find(|m| m.managed_rooms().contains(&room_id))
                    {
                        if let Ok(Some(room)) = manager
                            .create_for_peer(
                                peer_id.clone(),
                                role.clone(),
                                &room_id,
                                outbound_tx.clone(),
                            )
                            .await
                            && let Err(e) = builder.add_room(room_id.clone(), room)
                        {
                            tracing::warn!(
                                "Failed to add room {} for peer {}: {:?}",
                                room_id,
                                peer_id,
                                e
                            );
                        }
                    } else {
                        tracing::warn!("No manager found for negotiated room {}", room_id);
                    }
                }
            }

            // Build PeerChannels
            let peer_channels = match builder.build(inbound_rx).await {
                Ok(pc) => pc,
                Err(e) => return Err(format!("Failed to build PeerChannels: {:?}", e)),
            };

            // Register with Router
            {
                let mut router = router_arc.lock().await;
                if let Err(e) = router.register_peer(peer_channels) {
                    return Err(format!("Failed to register peer: {:?}", e));
                }
            }

            Ok(())
        })
    }
}

/// Handle peer disconnected event
#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct OnPeerDisconnected {
    /// The ID of the disconnected peer
    pub peer_id: PeerId,
}

// TODO: Add an integration test that simulates peer disconnection to cover this handler.
impl Handler<OnPeerDisconnected> for RouterActor {
    type Result = ResponseFuture<Result<(), String>>;

    fn handle(&mut self, msg: OnPeerDisconnected, _ctx: &mut Context<Self>) -> Self::Result {
        let router_arc = self.router.clone();
        let peer_id = msg.peer_id;

        Box::pin(async move {
            let mut router = router_arc.lock().await;
            router
                .disconnect_peer(&peer_id)
                .map_err(|e| format!("Failed to disconnect peer: {:?}", e))
        })
    }
}
