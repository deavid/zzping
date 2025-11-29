//! RouterActor - Actix wrapper for Router
//!
//! Provides actor-based interface for Router, enabling async messaging
//! from components and PeerManager.

use crate::router::Router;
use actix::prelude::*;
use zznet_api::{OnPeerConnected, RoomId};

/// RouterActor - Actix wrapper for Router
///
/// Provides message-based synchronous access to Router for components and PeerManager.
pub struct RouterActor {
    router: Router,
}

impl RouterActor {
    /// Create a new RouterActor
    pub fn new(offered_rooms: Vec<RoomId>) -> Self {
        Self {
            router: Router::new(offered_rooms),
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

/// Register a room factory with the router
#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct RegisterManager {
    /// The room factory to register
    pub factory: crate::RoomFactoryRef,
    /// The rooms this factory handles
    pub rooms: Vec<RoomId>,
}

impl Handler<RegisterManager> for RouterActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: RegisterManager, _ctx: &mut Context<Self>) -> Self::Result {
        self.router
            .register_manager(msg.factory, msg.rooms)
            .map_err(|e| format!("Failed to register manager: {:?}", e))
    }
}

impl Handler<OnPeerConnected> for RouterActor {
    type Result = Result<
        std::collections::HashMap<zznet_api::RoomId, zznet_api::RoomInboundRecipient>,
        String,
    >;

    fn handle(&mut self, msg: OnPeerConnected, _ctx: &mut Context<Self>) -> Self::Result {
        let peer_id = msg.peer_id.clone();
        let role = msg.role;
        let negotiated_rooms = msg.negotiated_rooms;
        let transport_tx = msg.transport_tx;
        let mut room_routes = std::collections::HashMap::new();

        // Get factories for negotiated rooms and call them synchronously
        for room_id in negotiated_rooms {
            if let Some(factory) = self.router.factories.get(&room_id) {
                match factory.create_room(
                    peer_id.clone(),
                    role.clone(),
                    room_id.clone(),
                    transport_tx.clone(),
                ) {
                    Ok(Some(room_recipient)) => {
                        // Store the room recipient for returning
                        tracing::debug!("RouterActor: created room route for peer={} room={}", peer_id, room_id);
                        room_routes.insert(room_id.clone(), room_recipient);
                    }
                    Ok(None) => {
                        tracing::warn!(
                            "Factory returned None for room {} for peer {}",
                            room_id,
                            peer_id
                        );
                    }
                    Err(e) => {
                        tracing::error!(
                            "Failed to create room {} for peer {}: {:?}",
                            room_id,
                            peer_id,
                            e
                        );
                    }
                }
            } else {
                tracing::warn!("No factory found for negotiated room {}", room_id);
            }
        }

        // NOTE: No longer spawning transport_demux - HelloActor handles inbound frames

        Ok(room_routes)
    }
}
