//! IntentConfig Network Manager Actor
//!
//! The Manager actor in the three-actor pattern.

use crate::internal_messages::{
    BroadcastConfigUpdate, InboundConfigChangeRequest, InboundGetConfigRequest,
    NetworkConfigChangeRequest, SendErrorToPeer,
};
use crate::messages::{GetCurrentConfig, IntentConfigData};
use crate::network_actor::IntentConfigNetworkActor;
use crate::network_messages::IntentConfigNetworkMsg;
use crate::permissions::IntentConfigPermissions;
use actix::prelude::*;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use zznet_api::types::{PeerId, RoomId};
use zznet_room::actor::RoomActor;
use zznet_room::room_manager::{CreateRoomForPeer, RoomInboundRecipient};
use zznet_router::RouterActor;

/// IntentConfigNetworkManager orchestrates the per-peer translator layer.
///
/// This actor bridges the business logic (`IntentConfigActor`) and the network stack
/// (`IntentConfigNetworkActor` + `RoomActor`). It subscribes to peer lifecycle events
/// and manages the fleet of translators attached to each peer.
///
/// # Responsibilities
/// - Lifecycle: Spawn/destroy translator actors as peers connect/disconnect
/// - Broadcast: Fan-out config updates to all connected peers
/// - Authorization: Handled by NetworkActors (they have the Role)
/// - Coordination: Aggregate responses from multiple peers when needed
///
/// # Message Flow
/// See `internal_messages.rs` for detailed message flow diagrams.
pub struct IntentConfigNetworkManager {
    /// Address of the main business logic actor
    main_actor: Addr<crate::actor::IntentConfigActor>,
    /// Per-peer translator actors for message translation
    translator_actors: Arc<RwLock<HashMap<PeerId, Addr<IntentConfigNetworkActor>>>>,
    /// Per-peer room actors for serialization
    room_actors: Arc<RwLock<HashMap<PeerId, Addr<RoomActor<IntentConfigNetworkMsg>>>>>,
    /// RouterActor for data-plane message routing
    router_actor: Addr<RouterActor>,
    /// Address of this NetworkManager (set in started())
    self_addr: Option<Addr<IntentConfigNetworkManager>>,
    /// Policy map from role strings to component-specific permissions
    permissions_map: HashMap<String, IntentConfigPermissions>,
}

impl Clone for IntentConfigNetworkManager {
    fn clone(&self) -> Self {
        Self {
            main_actor: self.main_actor.clone(),
            translator_actors: Arc::clone(&self.translator_actors),
            room_actors: Arc::clone(&self.room_actors),
            router_actor: self.router_actor.clone(),
            self_addr: self.self_addr.clone(),
            permissions_map: self.permissions_map.clone(),
        }
    }
}

impl IntentConfigNetworkManager {
    /// Create a new NetworkManager tied to the provided IntentConfigActor.
    pub fn new(
        main_actor: Addr<crate::actor::IntentConfigActor>,
        router_actor: Addr<RouterActor>,
        permissions_map: HashMap<String, IntentConfigPermissions>,
    ) -> Self {
        Self {
            main_actor,
            translator_actors: Arc::new(RwLock::new(HashMap::new())),
            room_actors: Arc::new(RwLock::new(HashMap::new())),
            router_actor,
            self_addr: None,
            permissions_map,
        }
    }

    /// Get the permissions map (for testing)
    pub fn permissions_map(&self) -> &HashMap<String, IntentConfigPermissions> {
        &self.permissions_map
    }

    /// Destroy per-peer actors when lifecycle events indicate removal.
    fn _destroy_peer_actors(&mut self, peer_id: &PeerId) {
        // FIXME: This code is dead. This is never executed which means we are missing tooling.
        let mut removed_translator = false;
        let mut removed_room_actor = false;

        if self
            .translator_actors
            .write()
            .unwrap()
            .remove(peer_id)
            .is_some()
        {
            removed_translator = true;
        }

        if self.room_actors.write().unwrap().remove(peer_id).is_some() {
            removed_room_actor = true;
        }

        if removed_translator || removed_room_actor {
            log::info!("Destroying actors for peer: {}", peer_id);
            log::debug!(
                "Actors destroyed - {} translators, {} room actors remain",
                self.translator_actors.read().unwrap().len(),
                self.room_actors.read().unwrap().len()
            );
        } else {
            log::warn!(
                "Attempted to destroy non-existent actors for peer: {}",
                peer_id
            );
        }
    }
}

// ============================================================================
// Actor Implementation
// ============================================================================

impl Actor for IntentConfigNetworkManager {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        log::info!("IntentConfigNetworkManager started");

        // Set our own address for RoomManager implementation
        let addr: Addr<IntentConfigNetworkManager> = ctx.address();
        self.self_addr = Some(addr);

        // Register ourselves as a RoomManager with the Router
        let manager = self
            .self_addr
            .as_ref()
            .unwrap()
            .clone()
            .recipient::<CreateRoomForPeer>();
        let rooms = vec![RoomId::from("intent-config")];
        let register_msg = zznet_router::RegisterManager { manager, rooms };
        self.router_actor.do_send(register_msg);

        // Keep actor alive
        ctx.set_mailbox_capacity(1000);
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        log::info!("IntentConfigNetworkManager stopped");

        // Clean up all actors
        let translator_count = self.translator_actors.read().unwrap().len();
        let room_count = self.room_actors.read().unwrap().len();
        self.translator_actors.write().unwrap().clear();
        self.room_actors.write().unwrap().clear();
        log::debug!(
            "Stopped {} NetworkActors and {} RoomActors",
            translator_count,
            room_count
        );
    }
}

// ============================================================================
// Handler: BroadcastConfigUpdate (from MainActor)
// ============================================================================

impl Handler<BroadcastConfigUpdate> for IntentConfigNetworkManager {
    type Result = ();

    fn handle(&mut self, msg: BroadcastConfigUpdate, _ctx: &mut Self::Context) -> Self::Result {
        log::info!(
            "Broadcasting config update to {} peers",
            self.room_actors.read().unwrap().len()
        );

        // Fan-out to all room actors
        for (peer_id, room_actor) in self.room_actors.read().unwrap().iter() {
            log::debug!("Sending config update to peer: {}", peer_id);
            let network_msg = IntentConfigNetworkMsg::ConfigUpdate {
                targets: msg.config.targets.clone(),
                ping_rate_pps: msg.config.ping_rate_pps,
            };
            room_actor.do_send(network_msg);
        }

        log::debug!("Broadcast complete");
    }
}

// ============================================================================
// Handler: SendErrorToPeer (from MainActor)
// ============================================================================

impl Handler<SendErrorToPeer> for IntentConfigNetworkManager {
    type Result = ();

    fn handle(&mut self, msg: SendErrorToPeer, _ctx: &mut Self::Context) -> Self::Result {
        if let Some(room_actor) = self.room_actors.read().unwrap().get(&msg.peer_id) {
            log::info!(
                "Sending error to peer {}: {}",
                msg.peer_id,
                msg.error_message
            );
            let network_msg = IntentConfigNetworkMsg::Error {
                reason: msg.error_message,
            };
            room_actor.do_send(network_msg);
        } else {
            log::warn!(
                "Cannot send error to peer {} - no RoomActor exists",
                msg.peer_id
            );
        }
    }
}

// ============================================================================
// Handler: InboundConfigChangeRequest (from NetworkActor)
// ============================================================================

impl Handler<InboundConfigChangeRequest> for IntentConfigNetworkManager {
    type Result = ();

    fn handle(
        &mut self,
        msg: InboundConfigChangeRequest,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        log::info!("Received config change request from peer: {}", msg.peer_id);

        // Authorization is now handled by NetworkActor (has the Role)
        // If we receive this message, the peer is already authorized

        let auth_request = NetworkConfigChangeRequest {
            peer_id: msg.peer_id,
            targets: msg.targets,
            ping_rate_pps: msg.ping_rate_pps,
            authorized: true,
        };

        if let Err(e) = self.main_actor.try_send(auth_request) {
            log::error!(
                "Failed to forward config change request to main actor: {}",
                e
            );
        }
    }
}

// ============================================================================
// Handler: InboundGetConfigRequest (from NetworkActor)
// ============================================================================

impl Handler<InboundGetConfigRequest> for IntentConfigNetworkManager {
    type Result = ResponseFuture<IntentConfigData>;

    fn handle(&mut self, msg: InboundGetConfigRequest, _ctx: &mut Self::Context) -> Self::Result {
        log::debug!("Received GetConfig request from peer: {}", msg.peer_id);

        let main_actor = self.main_actor.clone();

        Box::pin(async move {
            let result = main_actor.send(GetCurrentConfig).await;

            match result {
                Ok(config) => {
                    log::debug!("Returning config to peer: {}", msg.peer_id);
                    config
                }
                Err(e) => {
                    log::error!(
                        "Failed to get config from MainActor for peer {}: {}",
                        msg.peer_id,
                        e
                    );
                    // Return default config on error
                    IntentConfigData::default()
                }
            }
        })
    }
}

#[async_trait::async_trait]
impl Handler<CreateRoomForPeer> for IntentConfigNetworkManager {
    type Result = Result<Option<RoomInboundRecipient>, ()>;

    fn handle(&mut self, msg: CreateRoomForPeer, _ctx: &mut Context<Self>) -> Self::Result {
        // Only handle the "intent-config" room
        if msg.room_id != RoomId::from("intent-config") {
            return Ok(None);
        }

        // Translate the global Role to component-specific Permissions
        let permissions = self
            .permissions_map
            .get(msg.role.as_str())
            .cloned()
            .unwrap_or_default();

        // Create the translator actor with the peer's permissions (not role)
        let translator = IntentConfigNetworkActor::new(
            msg.peer_id.clone(),
            permissions,
            self.self_addr.as_ref().unwrap().clone(),
        );

        // Start the translator actor
        let translator_addr = translator.start();

        // Create the RoomActor<IntentConfigNetworkMsg>
        let room_actor = RoomActor::new(
            RoomId::from("intent-config"),
            msg.outbound_to_peer,
            translator_addr
                .clone()
                .recipient::<IntentConfigNetworkMsg>(),
        );

        // Start the RoomActor
        let room_actor_addr = room_actor.start();

        // Store the addresses in the maps
        {
            let mut translators = self.translator_actors.write().unwrap();
            translators.insert(msg.peer_id.clone(), translator_addr);

            let mut room_actors = self.room_actors.write().unwrap();
            room_actors.insert(msg.peer_id.clone(), room_actor_addr.clone());
        }

        // Return the RoomActor's raw inbound recipient
        let recipient = RoomActor::inbound_recipient(&room_actor_addr);

        Ok(Some(recipient))
    }
}

#[cfg(test)]
mod tests {
    // Phase 3.9 COMPLETE: Integration tests in tests/three_actor_integration_tests.rs
    // These tests cover the happy-path scenarios for the three-actor pattern.
    // Unit tests for NetworkManager are deferred until Room<T> integration is complete.
}
