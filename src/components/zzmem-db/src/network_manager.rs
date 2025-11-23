//! NetworkManager for MemDB: peer lifecycle and message routing.
//!
//! Implements the three-actor pattern: `MemDBActor` (main), `MemDBNetworkManager` (manager),
//! and `MemDBNetworkActor` (per-peer translator).

use actix::prelude::*;
use std::collections::HashMap;
use zznet_api::{PeerId, RoomId};
use zznet_room::RoomActor;
use zznet_router::RouterActor;

use crate::actor::MemDBActor;
use crate::events::MemDBEvent;
use crate::network_actor::{MemDBNetworkActor, SetRoomActor};
use crate::network_messages::MemDBMessage;
use crate::permissions::MemDBPermissions;

/// Factory that creates per-peer NetworkActors and RoomActors for MemDB.
pub(crate) struct MemDBRoomFactory {
    main_actor: Addr<MemDBActor>,
    _manager: Addr<MemDBNetworkManager>,
    event_bus: tokio::sync::broadcast::Sender<MemDBEvent>,
    permissions_map: HashMap<String, MemDBPermissions>,
}

impl MemDBRoomFactory {
    /// Create a new `MemDBRoomFactory`.
    pub(crate) fn new(
        main_actor: Addr<MemDBActor>,
        manager: Addr<MemDBNetworkManager>,
        event_bus: tokio::sync::broadcast::Sender<MemDBEvent>,
        permissions_map: HashMap<String, MemDBPermissions>,
    ) -> Self {
        Self {
            main_actor,
            _manager: manager,
            event_bus,
            permissions_map,
        }
    }
}

impl zznet_router::RoomFactory for MemDBRoomFactory {
    fn create_room(
        &self,
        peer_id: PeerId,
        role: zznet_api::Role,
        room_id: RoomId,
        transport_tx: tokio::sync::mpsc::Sender<zznet_api::TransportFrame>,
    ) -> Result<Option<zznet_room::RoomInboundRecipient>, String> {
        // Check if this is our room
        if room_id.as_str() != "memdb" {
            return Ok(None);
        }

        tracing::debug!(
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

        // Create NetworkActor
        let net = MemDBNetworkActor::new(
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
            net_addr.clone().recipient::<MemDBMessage>(),
        );
        let room_addr = room.start();

        // Provide the RoomActor address to the NetworkActor for outbound sends
        net_addr.do_send(SetRoomActor(room_addr.clone()));

        Ok(Some(room_addr.recipient()))
    }
}

/// `MemDBNetworkManager` supervises peer lifecycle and registration.
pub(crate) struct MemDBNetworkManager {
    /// Reference to the MainActor for business logic
    main_actor: Addr<MemDBActor>,

    /// RouterActor for data-plane message routing
    router_actor: Addr<RouterActor>,

    /// Event bus for outbound notifications from MainActor
    event_bus: tokio::sync::broadcast::Sender<MemDBEvent>,

    /// Permissions map for role-to-permissions translation
    permissions_map: HashMap<String, MemDBPermissions>,
}

impl Clone for MemDBNetworkManager {
    fn clone(&self) -> Self {
        Self {
            main_actor: self.main_actor.clone(),
            router_actor: self.router_actor.clone(),
            event_bus: self.event_bus.clone(),
            permissions_map: self.permissions_map.clone(),
        }
    }
}

impl MemDBNetworkManager {
    /// Create a new `MemDBNetworkManager`.
    pub(crate) fn new(
        main_actor: Addr<MemDBActor>,
        router_actor: Addr<RouterActor>,
        event_bus: tokio::sync::broadcast::Sender<MemDBEvent>,
        permissions_map: HashMap<String, MemDBPermissions>,
    ) -> Self {
        Self {
            main_actor,
            router_actor,
            event_bus,
            permissions_map,
        }
    }
}

impl Actor for MemDBNetworkManager {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        tracing::debug!("MemDBNetworkManager started");

        // Register with router using custom factory
        let factory = std::sync::Arc::new(MemDBRoomFactory::new(
            self.main_actor.clone(),
            ctx.address(),
            self.event_bus.clone(),
            self.permissions_map.clone(),
        ));
        let rooms = vec![RoomId::from("memdb")];
        let register_msg = zznet_router::RegisterManager { factory, rooms };
        self.router_actor.do_send(register_msg);
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        tracing::debug!("MemDBNetworkManager stopped");
    }
}
