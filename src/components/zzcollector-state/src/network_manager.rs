//! Network Manager for the CState component.
//!
//! This actor orchestrates peer lifecycle and manages per-peer NetworkActors.
//! It acts as a bridge between the business logic (CStateActor) and the network
//! layer (CStateNetworkActor instances).

use crate::{
    network_actor::{CStateNetworkActor, SetRoomActor},
    network_messages::CStateMessage,
    permissions::CStatePermissions,
};
use actix::prelude::*;
use log::{debug, info, warn};
use std::collections::HashMap;
use zznet_api::{PeerId, RoomId};
use zznet_room::RoomActor;
use zznet_router::{RoomFactory, RouterActor};

// ============================================================================
// Custom Room Factory
// ============================================================================

/// Custom RoomFactory for CState component.
///
/// Creates NetworkActors with event bus subscriptions and wires them to RoomActors
/// after creation to resolve circular dependencies (NetworkActor needs RoomActor,
/// RoomActor needs NetworkActor recipient).
pub(crate) struct CStateRoomFactory {
    main_actor: Addr<crate::actor::CStateActor>,
    _manager: Addr<CStateNetworkManager>,
    event_bus: tokio::sync::broadcast::Sender<crate::events::CStateEvent>,
    permissions_map: HashMap<String, CStatePermissions>,
}

impl CStateRoomFactory {
    /// Creates a new CStateRoomFactory.
    ///
    /// # Arguments
    /// * `main_actor` - Address of the MainActor for business logic
    /// * `manager` - Address of the NetworkManager for registration
    /// * `event_bus` - Event bus sender for broadcasting config changes
    /// * `permissions_map` - Map from role strings to permissions
    pub(crate) fn new(
        main_actor: Addr<crate::actor::CStateActor>,
        _manager: Addr<CStateNetworkManager>,
        event_bus: tokio::sync::broadcast::Sender<crate::events::CStateEvent>,
        permissions_map: HashMap<String, CStatePermissions>,
    ) -> Self {
        Self {
            main_actor,
            _manager,
            event_bus,
            permissions_map,
        }
    }
}

impl RoomFactory for CStateRoomFactory {
    fn create_room(
        &self,
        peer_id: PeerId,
        role: zznet_api::Role,
        room_id: RoomId,
        transport_tx: tokio::sync::mpsc::Sender<zznet_api::TransportFrame>,
    ) -> Result<Option<zznet_room::RoomInboundRecipient>, String> {
        // Check if this is our room
        if room_id.as_str() != "cstate" {
            return Ok(None);
        }

        debug!(
            "CStateRoomFactory: Creating room for peer {} with role {}",
            peer_id,
            role.as_str()
        );

        // Get permissions for role
        let permissions = self
            .permissions_map
            .get(role.as_str())
            .cloned()
            .unwrap_or_else(|| {
                warn!(
                    "No permissions found for role '{}', using default",
                    role.as_str()
                );
                CStatePermissions::default()
            });

        // Subscribe to event bus BEFORE creating NetworkActor
        let event_rx = self.event_bus.subscribe();

        let network_actor = CStateNetworkActor::new(
            peer_id.clone(),
            permissions,
            self.main_actor.clone(),
            event_rx,
        );
        let network_addr = network_actor.start();

        let room = RoomActor::new(
            room_id,
            transport_tx,
            network_addr.clone().recipient::<CStateMessage>(),
        );
        let room_addr = room.start();

        network_addr.do_send(SetRoomActor(room_addr.clone()));

        Ok(Some(room_addr.recipient()))
    }
}

/// The NetworkManager orchestrates peer lifecycle and manages NetworkActors.
///
/// Responsibilities:
/// - Subscribe to PeerLifecycleEvents from Router
/// - Spawn/stop CStateNetworkActor per connected peer
/// - Register Room<CStateMessage> with Router
/// - Route outbound messages to appropriate NetworkActors
/// - Handle peer disconnection cleanup
pub(crate) struct CStateNetworkManager {
    /// The router actor for sending messages
    router: Addr<RouterActor>,
    /// The main actor for handling internal messages
    main_actor: Addr<crate::actor::CStateActor>,
    /// Event bus for broadcasting heartbeats
    event_bus: tokio::sync::broadcast::Sender<crate::events::CStateEvent>,
    /// Permissions map for role-to-permissions translation
    permissions_map: HashMap<String, CStatePermissions>,
}

impl Clone for CStateNetworkManager {
    fn clone(&self) -> Self {
        Self {
            router: self.router.clone(),
            main_actor: self.main_actor.clone(),
            event_bus: self.event_bus.clone(),
            permissions_map: self.permissions_map.clone(),
        }
    }
}

impl CStateNetworkManager {
    /// Creates a new CStateNetworkManager.
    pub(crate) fn new(
        main_actor: Addr<crate::actor::CStateActor>,
        router: Addr<RouterActor>,
        event_bus: tokio::sync::broadcast::Sender<crate::events::CStateEvent>,
        permissions_map: HashMap<String, CStatePermissions>,
    ) -> Self {
        Self {
            router,
            main_actor,
            event_bus,
            permissions_map,
        }
    }
}

impl Actor for CStateNetworkManager {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("CStateNetworkManager started");

        // Use custom factory with event_bus
        let factory = std::sync::Arc::new(CStateRoomFactory::new(
            self.main_actor.clone(),
            ctx.address(),
            self.event_bus.clone(),
            self.permissions_map.clone(),
        ));
        let rooms = vec![RoomId::from("cstate")];
        let register_msg = zznet_router::RegisterManager { factory, rooms };
        self.router.do_send(register_msg);

        // Keep actor alive
        ctx.set_mailbox_capacity(1000);
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("CStateNetworkManager stopped");
        // Actors will be automatically stopped when dropped
    }
}
