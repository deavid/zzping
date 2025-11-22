//! IntentConfig Network Manager Actor
//!
//! The Manager actor in the three-actor pattern.

use crate::events::IntentConfigEvent;
use crate::internal_messages::{
    InboundConfigChangeRequest, InboundGetConfigRequest, NetworkConfigChangeRequest,
};
use crate::messages::{GetCurrentConfig, IntentConfigData};
use crate::network_actor::IntentConfigNetworkActor;
use crate::network_messages::IntentConfigNetworkMsg;
use crate::permissions::IntentConfigPermissions;
use actix::prelude::*;
use std::collections::HashMap;
use zznet_api::types::{PeerId, RoomId};
use zznet_room::actor::RoomActor;
use zznet_router::RouterActor;

/// Custom factory for IntentConfig that passes event_bus to NetworkActors
///
/// Replaces StandardRoomFactory to resolve circular dependency.
/// Creates NetworkActor first, then RoomActor, then wires them via SetRoomActor.
pub struct IntentConfigRoomFactory {
    manager: Addr<IntentConfigNetworkManager>,
    event_bus: tokio::sync::broadcast::Sender<IntentConfigEvent>,
    permissions_map: HashMap<String, IntentConfigPermissions>,
}

impl IntentConfigRoomFactory {
    /// Create a new IntentConfigRoomFactory
    ///
    /// # Arguments
    /// * `manager` - Address of the NetworkManager for registration
    /// * `event_bus` - Event bus sender for broadcasting config changes
    /// * `permissions_map` - Map from role strings to permissions
    pub fn new(
        manager: Addr<IntentConfigNetworkManager>,
        event_bus: tokio::sync::broadcast::Sender<IntentConfigEvent>,
        permissions_map: HashMap<String, IntentConfigPermissions>,
    ) -> Self {
        Self {
            manager,
            event_bus,
            permissions_map,
        }
    }
}

impl zznet_router::RoomFactory for IntentConfigRoomFactory {
    fn create_room(
        &self,
        peer_id: PeerId,
        role: zznet_api::types::Role,
        room_id: RoomId,
        transport_tx: tokio::sync::mpsc::Sender<zznet_api::types::TransportFrame>,
    ) -> Result<Option<zznet_room::room_manager::RoomInboundRecipient>, String> {
        // Check if this is our room
        if room_id.as_str() != "intent-config" {
            return Ok(None);
        }

        log::debug!(
            "Creating room for peer {} with role {}",
            peer_id,
            role.as_str()
        );

        // Lookup permissions
        let perms = self
            .permissions_map
            .get(role.as_str())
            .cloned()
            .unwrap_or_default();

        // Create NetworkActor without room_actor
        let net = IntentConfigNetworkActor::new(
            peer_id.clone(),
            perms,
            self.manager.clone(),
            self.event_bus.subscribe(), // Each actor gets its own Receiver
        );
        let net_addr = net.start();

        // Create RoomActor with NetworkActor's recipient
        let room = RoomActor::new(
            room_id,
            transport_tx,
            net_addr.clone().recipient::<IntentConfigNetworkMsg>(),
        );
        let room_addr = room.start();

        // Wire them together via SetRoomActor message
        net_addr.do_send(crate::network_actor::SetRoomActor(room_addr.clone()));

        Ok(Some(room_addr.recipient()))
    }
}

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
    /// Event bus for broadcasting config changes to all NetworkActors
    event_bus: tokio::sync::broadcast::Sender<IntentConfigEvent>,
    /// RouterActor for data-plane message routing
    router_actor: Addr<RouterActor>,
    /// Policy map from role strings to component-specific permissions
    permissions_map: HashMap<String, IntentConfigPermissions>,
}

impl Clone for IntentConfigNetworkManager {
    fn clone(&self) -> Self {
        Self {
            main_actor: self.main_actor.clone(),
            event_bus: self.event_bus.clone(),
            router_actor: self.router_actor.clone(),
            permissions_map: self.permissions_map.clone(),
        }
    }
}

impl IntentConfigNetworkManager {
    /// Create a new NetworkManager tied to the provided IntentConfigActor.
    ///
    /// The event_bus will be created with a default channel. After starting,
    /// you should call update_event_bus() to set it to the MainActor's actual event_bus.
    pub fn new(
        main_actor: Addr<crate::actor::IntentConfigActor>,
        router_actor: Addr<RouterActor>,
        permissions_map: HashMap<String, IntentConfigPermissions>,
    ) -> Self {
        let (event_tx, _) = tokio::sync::broadcast::channel(100);
        Self {
            main_actor,
            event_bus: event_tx,
            router_actor,
            permissions_map,
        }
    }

    /// Update the event bus to use the one from MainActor
    pub fn set_event_bus(&mut self, event_bus: tokio::sync::broadcast::Sender<IntentConfigEvent>) {
        self.event_bus = event_bus;
    }

    /// Get the permissions map (for testing)
    pub fn permissions_map(&self) -> &HashMap<String, IntentConfigPermissions> {
        &self.permissions_map
    }
}

// ============================================================================
// Actor Implementation
// ============================================================================

impl Actor for IntentConfigNetworkManager {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        log::info!("IntentConfigNetworkManager started");

        // Register with router using custom factory
        let factory = std::sync::Arc::new(IntentConfigRoomFactory::new(
            ctx.address(),
            self.event_bus.clone(),
            self.permissions_map.clone(),
        ));
        let rooms = vec![RoomId::from("intent-config")];
        let register_msg = zznet_router::RegisterManager { factory, rooms };
        self.router_actor.do_send(register_msg);

        // Keep actor alive
        ctx.set_mailbox_capacity(1000);
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        log::info!("IntentConfigNetworkManager stopped");

        // Actors will be automatically stopped when dropped
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
