//! RouterActor - Actix wrapper for Router
//!
//! Provides actor-based interface for Router, enabling async messaging
//! from components and PeerManager.

use crate::router::Router;
use actix::prelude::*;
use std::sync::Arc;
use zznet_api::messages::OnPeerConnected;
use zznet_api::types::{PeerId, RoomId};

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
    /// The room manager recipient to register
    pub manager: actix::Recipient<zznet_room::room_manager::CreateRoomForPeer>,
    /// The rooms this manager handles
    pub rooms: Vec<RoomId>,
}

impl Handler<RegisterManager> for RouterActor {
    type Result = ResponseFuture<Result<(), String>>;

    fn handle(&mut self, msg: RegisterManager, _ctx: &mut Context<Self>) -> Self::Result {
        let router_arc = self.router.clone();
        let manager = msg.manager;
        let rooms = msg.rooms;

        Box::pin(async move {
            let mut router = router_arc.lock().await;
            router
                .register_manager(manager, rooms)
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
        let mut builder = crate::peer_channels::PeerChannelsBuilder::new(peer_id.clone());

        Box::pin(async move {
            let managers_to_call = {
                let router = router_arc.lock().await;
                let mut list = Vec::new();
                for room_id in negotiated_rooms {
                    if let Some(recipient) = router.managers.get(&room_id) {
                        list.push((room_id, recipient.clone()));
                    } else {
                        tracing::warn!("No manager found for negotiated room {}", room_id);
                    }
                }
                list
            };

            for (room_id, recipient) in managers_to_call {
                let msg = zznet_room::room_manager::CreateRoomForPeer {
                    peer_id: peer_id.clone(),
                    role: role.clone(),
                    room_id: room_id.clone(),
                    outbound_to_peer: outbound_tx.clone(),
                };
                match recipient.send(msg).await {
                    Ok(Ok(Some(room))) => {
                        if let Err(e) = builder.add_room(room_id.clone(), room) {
                            tracing::warn!(
                                "Failed to add room {} for peer {}: {:?}",
                                room_id,
                                peer_id,
                                e
                            );
                        }
                    }
                    Ok(Ok(None)) => {
                        tracing::warn!(
                            "Manager returned None for room {} for peer {}",
                            room_id,
                            peer_id
                        );
                    }
                    Ok(Err(e)) => {
                        tracing::warn!(
                            "Failed to create room {} for peer {}: {:?}",
                            room_id,
                            peer_id,
                            e
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to send CreateRoomForPeer for room {} to peer {}: {:?}",
                            room_id,
                            peer_id,
                            e
                        );
                    }
                }
            }

            {
                let peer_channels = builder.build(inbound_rx);
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
