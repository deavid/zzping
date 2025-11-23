//! IntentConfig Network Manager Actor
//!
//! The Manager actor in the three-actor pattern.

use crate::actor::IntentConfigActor;
use crate::events::IntentConfigEvent;
use crate::network_actor::IntentConfigNetworkActor;
use crate::network_messages::IntentConfigNetworkMsg;
use crate::permissions::IntentConfigPermissions;
use actix::prelude::*;
use std::collections::HashMap;
use zznet_api::{PeerId, RoomId};
use zznet_room::RoomActor;
use zznet_router::RouterActor;

/// Custom factory for IntentConfig that passes event_bus to NetworkActors
///
/// Replaces StandardRoomFactory to resolve circular dependency.
/// Creates NetworkActor first, then RoomActor, then wires them via SetRoomActor.
pub(crate) struct IntentConfigRoomFactory {
    main_actor: Addr<IntentConfigActor>,
    _manager: Addr<IntentConfigNetworkManager>,
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
    pub(crate) fn new(
        main_actor: Addr<IntentConfigActor>,
        manager: Addr<IntentConfigNetworkManager>,
        event_bus: tokio::sync::broadcast::Sender<IntentConfigEvent>,
        permissions_map: HashMap<String, IntentConfigPermissions>,
    ) -> Self {
        Self {
            main_actor,
            _manager: manager,
            event_bus,
            permissions_map,
        }
    }
}

impl zznet_router::RoomFactory for IntentConfigRoomFactory {
    fn create_room(
        &self,
        peer_id: PeerId,
        role: zznet_api::Role,
        room_id: RoomId,
        transport_tx: tokio::sync::mpsc::Sender<zznet_api::TransportFrame>,
    ) -> Result<Option<zznet_room::RoomInboundRecipient>, String> {
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
            self.main_actor.clone(),
            self.event_bus.subscribe(),
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
pub(crate) struct IntentConfigNetworkManager {
    /// Address of the main business logic actor
    main_actor: Addr<IntentConfigActor>,
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
    /// The caller must supply the MainActor's event bus so that NetworkActors
    /// receive real-time config updates without extra wiring.
    pub(crate) fn new(
        main_actor: Addr<IntentConfigActor>,
        router_actor: Addr<RouterActor>,
        event_bus: tokio::sync::broadcast::Sender<IntentConfigEvent>,
        permissions_map: HashMap<String, IntentConfigPermissions>,
    ) -> Self {
        Self {
            main_actor,
            event_bus,
            router_actor,
            permissions_map,
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

        // Register with router using custom factory
        let factory = std::sync::Arc::new(IntentConfigRoomFactory::new(
            self.main_actor.clone(),
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
