//! IntentConfig Network Manager Actor
//!
//! The Manager actor in the three-actor pattern. Responsibilities:
//! - Subscribe to PeerLifecycleEvent bus from zznet-peer-manager
//! - Spawn `IntentConfigTranslatorActor` when peers join the "intent-config" room
//! - Destroy per-peer actors when peers disconnect
//! - Handle broadcast requests from MainActor (`BroadcastConfigUpdate`)
//! - Forward inbound requests to MainActor (after authorization check)
//! - Query PeerManager for role/permission checks

use crate::internal_messages::{
    BroadcastConfigUpdate, InboundConfigChangeRequest, InboundGetConfigRequest,
    NetworkConfigChangeRequest, SendErrorToPeer,
};
use crate::messages::{GetCurrentConfig, IntentConfigData};
use crate::network_messages::IntentConfigNetworkMsg;
use crate::translator_actor::IntentConfigTranslatorActor;
use actix::prelude::*;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

// Phase 7.2: Use traits for interface segregation (control-plane vs data-plane)
use zznet_api::types::{PeerId, PeerLifecycleEvent, Permission, RoomId};
use zznet_peer_manager::PeerManagerActor;
use zznet_room::actor::RoomActor;
use zznet_room::room_manager::{CreateError, RoomInboundRecipient, RoomManager};
use zznet_router::RouterActor;

/// IntentConfigNetworkManager orchestrates the per-peer translator layer.
///
/// This actor bridges the business logic (`IntentConfigActor`) and the network stack
/// (`IntentConfigTranslatorActor` + `RoomActor`). It subscribes to peer lifecycle events
/// and manages the fleet of translators attached to each peer.
///
/// # Responsibilities
/// - Lifecycle: Spawn/destroy translator actors as peers connect/disconnect
/// - Broadcast: Fan-out config updates to all connected peers
/// - Authorization: Query PeerManager for role/permission checks before forwarding requests
/// - Coordination: Aggregate responses from multiple peers when needed
///
/// # Message Flow
/// See `internal_messages.rs` for detailed message flow diagrams.
pub struct IntentConfigNetworkManager {
    /// Address of the main business logic actor
    main_actor: Addr<crate::actor::IntentConfigActor>,
    /// Per-peer translator actors for message translation
    translator_actors: Arc<RwLock<HashMap<PeerId, Addr<IntentConfigTranslatorActor>>>>,
    /// Per-peer room actors for serialization
    room_actors: Arc<RwLock<HashMap<PeerId, Addr<RoomActor<IntentConfigNetworkMsg>>>>>,
    /// PeerManagerActor for control-plane queries
    peer_manager: Addr<PeerManagerActor>,
    /// RouterActor for data-plane message routing
    router_actor: Addr<RouterActor>,
    /// Address of this NetworkManager (set in started())
    self_addr: Option<Addr<IntentConfigNetworkManager>>,
}

impl Clone for IntentConfigNetworkManager {
    fn clone(&self) -> Self {
        Self {
            main_actor: self.main_actor.clone(),
            translator_actors: Arc::clone(&self.translator_actors),
            room_actors: Arc::clone(&self.room_actors),
            peer_manager: self.peer_manager.clone(),
            router_actor: self.router_actor.clone(),
            self_addr: self.self_addr.clone(),
        }
    }
}

impl IntentConfigNetworkManager {
    /// Create a new NetworkManager tied to the provided IntentConfigActor.
    pub fn new(
        main_actor: Addr<crate::actor::IntentConfigActor>,
        peer_manager: Addr<PeerManagerActor>,
        router_actor: Addr<RouterActor>,
    ) -> Self {
        Self {
            main_actor,
            translator_actors: Arc::new(RwLock::new(HashMap::new())),
            room_actors: Arc::new(RwLock::new(HashMap::new())),
            peer_manager,
            router_actor,
            self_addr: None,
        }
    }

    /// Destroy per-peer actors when lifecycle events indicate removal.
    fn destroy_peer_actors(&mut self, peer_id: &PeerId) {
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
        let manager =
            std::sync::Arc::new(self.clone()) as std::sync::Arc<dyn RoomManager + Send + Sync>;
        let register_msg = zznet_router::RegisterManager { manager };
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
            "Stopped {} TranslatorActors and {} RoomActors",
            translator_count,
            room_count
        );
    }
}

// ============================================================================
// Handler: PeerLifecycleEvent (wrapped for actix compatibility)
// ============================================================================

/// Wrapper to make PeerLifecycleEvent work with actix Handler
#[derive(Message, Debug, Clone)]
#[rtype(result = "()")]
pub struct PeerLifecycleEventWrapper(pub PeerLifecycleEvent);

impl Handler<PeerLifecycleEventWrapper> for IntentConfigNetworkManager {
    type Result = ();

    fn handle(&mut self, msg: PeerLifecycleEventWrapper, _ctx: &mut Self::Context) -> Self::Result {
        match msg.0 {
            PeerLifecycleEvent::PeerAdded { peer_id } => {
                log::debug!("PeerLifecycleEvent::PeerAdded: {}", peer_id);
                // Actors are now created in create_for_peer when Router calls it
            }
            PeerLifecycleEvent::PeerConnected { peer_id } => {
                log::debug!("PeerLifecycleEvent::PeerConnected: {}", peer_id);
                // Translator actor already created on PeerAdded
            }
            PeerLifecycleEvent::PeerDisconnected { peer_id } => {
                log::debug!("PeerLifecycleEvent::PeerDisconnected: {}", peer_id);
                self.destroy_peer_actors(&peer_id);
            }
            PeerLifecycleEvent::PeerRemoved { peer_id } => {
                log::debug!("PeerLifecycleEvent::PeerRemoved: {}", peer_id);
                self.destroy_peer_actors(&peer_id);
            }
            PeerLifecycleEvent::PeerIdentityUpdated { peer_id, identity } => {
                log::debug!(
                    "PeerLifecycleEvent::PeerIdentityUpdated: {} - {:?}",
                    peer_id,
                    identity
                );
                // Identity updates don't affect IntentConfig translator actors
                // Authorization is checked on each request, not cached
            }
        }
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
// Handler: InboundConfigChangeRequest (from TranslatorActor)
// ============================================================================

impl Handler<InboundConfigChangeRequest> for IntentConfigNetworkManager {
    type Result = ();

    fn handle(&mut self, msg: InboundConfigChangeRequest, ctx: &mut Self::Context) -> Self::Result {
        log::info!("Received config change request from peer: {}", msg.peer_id);

        let main_actor = self.main_actor.clone();
        let peer_id = msg.peer_id.clone();
        let targets = msg.targets;
        let ping_rate_pps = msg.ping_rate_pps;
        let manager_addr: Addr<IntentConfigNetworkManager> = ctx.address();

        // Check authorization using PeerManagerActor
        let rt = tokio::runtime::Handle::current();
        let role_future = self.peer_manager.send(zznet_peer_manager::GetPeerRole {
            peer_id: peer_id.clone(),
        });
        let role = rt.block_on(role_future).unwrap_or(None);
        let authorized = match role {
            Some(role) => {
                let is_admin = role.as_str() == "client-admin";
                if !is_admin {
                    log::warn!(
                        "Peer {} has role '{}' - not authorized (requires 'client-admin')",
                        peer_id,
                        role.as_str()
                    );
                }
                is_admin
            }
            None => {
                log::warn!("Peer {} has no role - not authorized", peer_id);
                false
            }
        };

        if !authorized {
            log::warn!(
                "Config change request from peer {} DENIED - insufficient permissions",
                peer_id
            );

            // Send error back to peer
            manager_addr.do_send(SendErrorToPeer {
                peer_id: peer_id.clone(),
                error_message: "Unauthorized: ClientAdmin role required".to_string(),
            });
            return;
        }

        // Forward to MainActor for processing
        log::debug!(
            "Config change request from peer {} AUTHORIZED - forwarding to MainActor",
            peer_id
        );

        let auth_request = NetworkConfigChangeRequest {
            peer_id,
            targets,
            ping_rate_pps,
            authorized: true,
        };

        if let Err(e) = main_actor.try_send(auth_request) {
            log::error!(
                "Failed to forward config change request to main actor: {}",
                e
            );
        }
    }
}

// ============================================================================
// Handler: InboundGetConfigRequest (from TranslatorActor)
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
impl RoomManager for IntentConfigNetworkManager {
    fn managed_rooms(&self) -> HashSet<RoomId> {
        let mut rooms = HashSet::new();
        rooms.insert(RoomId::from("intent-config"));
        rooms
    }

    async fn create_for_peer(
        &self,
        peer_id: PeerId,
        _permission: Permission,
        room_id: &RoomId,
        outbound_to_peer: tokio::sync::mpsc::Sender<(zznet_api::types::RoomId, Vec<u8>)>,
    ) -> Result<Option<RoomInboundRecipient>, CreateError> {
        // Only handle the "intent-config" room
        if room_id != &RoomId::from("intent-config") {
            return Ok(None);
        }

        // Create the translator actor
        let translator = IntentConfigTranslatorActor::new(
            peer_id.clone(),
            self.self_addr.as_ref().unwrap().clone(),
        );

        // Start the translator actor
        let translator_addr = translator.start();

        // Create the RoomActor<IntentConfigNetworkMsg>
        let room_actor = RoomActor::new(
            RoomId::from("intent-config"),
            outbound_to_peer,
            translator_addr
                .clone()
                .recipient::<IntentConfigNetworkMsg>(),
        );

        // Start the RoomActor
        let room_actor_addr = room_actor.start();

        // Store the addresses in the maps
        {
            let mut translators = self.translator_actors.write().unwrap();
            translators.insert(peer_id.clone(), translator_addr);

            let mut room_actors = self.room_actors.write().unwrap();
            room_actors.insert(peer_id, room_actor_addr.clone());
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
