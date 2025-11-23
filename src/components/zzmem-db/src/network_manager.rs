//! NetworkManager for MemDB: peer lifecycle and message routing.
//!
//! Implements the three-actor pattern: `MemDBActor` (main), `MemDBNetworkManager` (manager),
//! and `MemDBNetworkActor` (per-peer translator).

use actix::prelude::*;
use std::collections::HashMap;
use zznet_api::types::{PeerId, RoomId};
use zznet_room::actor::RoomActor;
use zznet_router::RouterActor;

use crate::actor::MemDBActor;
use crate::network_actor::MemDBNetworkActor;
use crate::network_messages::MemDBMessage;
use crate::permissions::MemDBPermissions;

/// Factory that creates per-peer NetworkActors and RoomActors for MemDB.
pub struct MemDBRoomFactory {
    main_actor: Addr<MemDBActor>,
    _manager: Addr<MemDBNetworkManager>,
    permissions_map: HashMap<String, MemDBPermissions>,
}

impl MemDBRoomFactory {
    /// Create a new `MemDBRoomFactory`.
    pub fn new(
        main_actor: Addr<MemDBActor>,
        manager: Addr<MemDBNetworkManager>,
        permissions_map: HashMap<String, MemDBPermissions>,
    ) -> Self {
        Self {
            main_actor,
            _manager: manager,
            permissions_map,
        }
    }
}

impl zznet_router::RoomFactory for MemDBRoomFactory {
    fn create_room(
        &self,
        peer_id: PeerId,
        role: zznet_api::types::Role,
        room_id: RoomId,
        transport_tx: tokio::sync::mpsc::Sender<zznet_api::types::TransportFrame>,
    ) -> Result<Option<zznet_room::room_manager::RoomInboundRecipient>, String> {
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
        let net = MemDBNetworkActor::new(peer_id.clone(), perms, self.main_actor.clone());
        let net_addr = net.start();

        // Create RoomActor with NetworkActor's recipient
        let room = RoomActor::new(
            room_id,
            transport_tx,
            net_addr.clone().recipient::<MemDBMessage>(),
        );
        let room_addr = room.start();

        // No wiring needed - MemDBNetworkActor doesn't need room_actor reference

        Ok(Some(room_addr.recipient()))
    }
}

/// `MemDBNetworkManager` supervises peer lifecycle and registration.
pub struct MemDBNetworkManager {
    /// Reference to the MainActor for business logic
    main_actor: Addr<MemDBActor>,

    /// RouterActor for data-plane message routing
    router_actor: Addr<RouterActor>,

    /// Permissions map for role-to-permissions translation
    permissions_map: HashMap<String, MemDBPermissions>,
}

impl Clone for MemDBNetworkManager {
    fn clone(&self) -> Self {
        Self {
            main_actor: self.main_actor.clone(),
            router_actor: self.router_actor.clone(),
            permissions_map: self.permissions_map.clone(),
        }
    }
}

impl MemDBNetworkManager {
    /// Create a new `MemDBNetworkManager`.
    pub fn new(
        main_actor: Addr<MemDBActor>,
        router_actor: Addr<RouterActor>,
        permissions_map: HashMap<String, MemDBPermissions>,
    ) -> Self {
        Self {
            main_actor,
            router_actor,
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

// Batch transmission handler: broadcast prepared batches to Database peers.

impl Handler<crate::internal_messages::BatchReadyToSend> for MemDBNetworkManager {
    type Result = ();

    fn handle(
        &mut self,
        msg: crate::internal_messages::BatchReadyToSend,
        _ctx: &mut Context<Self>,
    ) -> Self::Result {
        // Prepare network message and log readiness; broadcasting via RouterActor is TODO.
        let results_count = msg.results.len();
        let _network_msg = MemDBMessage::SubmitBatch {
            sender_peer_id: "collector".to_string(),
            timestamp_ms: msg.timestamp_ms,
            results: msg.results,
        };

        tracing::info!(
            "Batch transmission prepared: timestamp={}, results_count={}",
            msg.timestamp_ms,
            results_count
        );
    }
}
