//! RouterActor - Actix wrapper for Router
//!
//! Provides actor-based interface for Router, enabling async messaging
//! from components and PeerManager.

use actix::prelude::*;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};
use zznet_api::types::{PeerId, Role, RoomId};
use zznet_room::room_manager::RoomManager;

use crate::router::Router;

/// RouterActor - Actix wrapper for Router
///
/// Provides message-based async access to Router for components and PeerManager.
pub struct RouterActor {
    router: Arc<tokio::sync::Mutex<Router>>,
}

impl RouterActor {
    /// Create a new RouterActor
    pub fn new(offered_rooms: Vec<RoomId>, max_rooms_per_peer: Option<usize>) -> Self {
        Self {
            router: Arc::new(tokio::sync::Mutex::new(Router::new(
                offered_rooms,
                max_rooms_per_peer,
            ))),
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

/// Handle peer connected event from PeerManager
#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct OnPeerConnected {
    /// The ID of the connected peer
    pub peer_id: PeerId,
    /// The role of the peer
    pub role: Role,
    /// Sender for outbound messages to the peer
    pub outbound_tx: mpsc::Sender<(RoomId, Vec<u8>)>,
    /// Receiver for inbound messages from the peer
    pub inbound_rx: mpsc::Receiver<(RoomId, Vec<u8>)>,
}

impl Handler<OnPeerConnected> for RouterActor {
    type Result = ResponseFuture<Result<(), String>>;

    fn handle(&mut self, msg: OnPeerConnected, _ctx: &mut Context<Self>) -> Self::Result {
        let router_arc = self.router.clone();
        let peer_id = msg.peer_id.clone();
        let role = msg.role;
        let outbound_tx = msg.outbound_tx;
        let inbound_rx = msg.inbound_rx;

        Box::pin(async move {
            // Gather rooms from managers
            let mut builder = crate::peer_channels::PeerChannelsBuilder::new(peer_id.clone());
            {
                let router = router_arc.lock().await;
                for manager in router.managers.values() {
                    for room_id in manager.managed_rooms() {
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
                    }
                }
            }

            // Build PeerChannels
            let peer_channels = match builder.build(outbound_tx, inbound_rx).await {
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

/// Handle PublishRooms from peer
#[derive(Message)]
#[rtype(result = "Result<Vec<RoomId>, String>")]
pub struct HandlePublishRooms {
    /// The ID of the peer sending the rooms
    pub peer_id: PeerId,
    /// The list of rooms offered by the peer
    pub peer_rooms: Vec<RoomId>,
}

impl Handler<HandlePublishRooms> for RouterActor {
    type Result = ResponseFuture<Result<Vec<RoomId>, String>>;

    fn handle(&mut self, msg: HandlePublishRooms, _ctx: &mut Context<Self>) -> Self::Result {
        let router_arc = self.router.clone();
        let peer_id = msg.peer_id;
        let peer_rooms = msg.peer_rooms;

        Box::pin(async move {
            let mut router = router_arc.lock().await;
            router
                .handle_publish_rooms(&peer_id, peer_rooms)
                .await
                .map_err(|e| format!("Failed to handle publish rooms: {:?}", e))
        })
    }
}

/// Query peer joined rooms
#[derive(Message)]
#[rtype(result = "Result<Vec<RoomId>, String>")]
pub struct PeerJoinedRooms {
    /// The ID of the peer to query
    pub peer_id: PeerId,
}

impl Handler<PeerJoinedRooms> for RouterActor {
    type Result = ResponseFuture<Result<Vec<RoomId>, String>>;

    fn handle(&mut self, msg: PeerJoinedRooms, _ctx: &mut Context<Self>) -> Self::Result {
        let router_arc = self.router.clone();
        let peer_id = msg.peer_id;

        Box::pin(async move {
            let router = router_arc.lock().await;
            router
                .peer_joined_rooms(&peer_id)
                .await
                .map_err(|e| format!("Failed to get joined rooms: {:?}", e))
        })
    }
}

/// Check if room joined
#[derive(Message)]
#[rtype(result = "Result<bool, String>")]
pub struct IsRoomJoined {
    /// The ID of the peer
    pub peer_id: PeerId,
    /// The ID of the room to check
    pub room_id: RoomId,
}

impl Handler<IsRoomJoined> for RouterActor {
    type Result = ResponseFuture<Result<bool, String>>;

    fn handle(&mut self, msg: IsRoomJoined, _ctx: &mut Context<Self>) -> Self::Result {
        let router_arc = self.router.clone();
        let peer_id = msg.peer_id;
        let room_id = msg.room_id;

        Box::pin(async move {
            let router = router_arc.lock().await;
            router
                .is_room_joined(&peer_id, &room_id)
                .await
                .map_err(|e| format!("Failed to check room joined: {:?}", e))
        })
    }
}

/// Get peer sender
#[derive(Message)]
#[rtype(result = "Option<mpsc::Sender<(RoomId, Vec<u8>)>>")]
pub struct PeerSender {
    /// The ID of the peer
    pub peer_id: PeerId,
}

impl Handler<PeerSender> for RouterActor {
    type Result = ResponseFuture<Option<mpsc::Sender<(RoomId, Vec<u8>)>>>;

    fn handle(&mut self, msg: PeerSender, _ctx: &mut Context<Self>) -> Self::Result {
        let router_arc = self.router.clone();
        let peer_id = msg.peer_id;

        Box::pin(async move {
            let router = router_arc.lock().await;
            router.peer_sender(&peer_id).ok().flatten()
        })
    }
}

/// Subscribe to peer inbound
#[derive(Message)]
#[rtype(result = "Option<broadcast::Receiver<(RoomId, Vec<u8>)>>")]
pub struct SubscribePeerInbound {
    /// The ID of the peer to subscribe to
    pub peer_id: PeerId,
}

impl Handler<SubscribePeerInbound> for RouterActor {
    type Result = ResponseFuture<Option<broadcast::Receiver<(RoomId, Vec<u8>)>>>;

    fn handle(&mut self, msg: SubscribePeerInbound, _ctx: &mut Context<Self>) -> Self::Result {
        let router_arc = self.router.clone();
        let peer_id = msg.peer_id;

        Box::pin(async move {
            let router = router_arc.lock().await;
            router.subscribe_peer_inbound(&peer_id).ok().flatten()
        })
    }
}
