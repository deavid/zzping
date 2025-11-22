//! NetworkManager for MemDB component - orchestrates peer lifecycle and message routing.
//!
//! This module implements the NetworkManager actor in the three-actor pattern:
//! - **MainActor** (MemDBActor): Pure business logic, zero network dependencies
//! - **NetworkManager** (this file): Peer lifecycle, message routing orchestration
//! - **NetworkActor**: Per-peer protocol translation
//!
//! ## Responsibilities
//!
//! 1. **Peer Lifecycle Management**:
//!    - Spawns MemDBNetworkActor when peer joins
//!    - Removes NetworkActor when peer leaves
//!    - Tracks all active peer connections
//!
//! 2. **Message Routing**:
//!    - Routes SendBatchAck to specific collector peer
//!    - Routes SendQueryResponse to specific admin peer
//!    - Routes SendSubmitBatch to database peer
//!
//! 3. **Room Management**:
//!    - Stores Room<MemDBMessage> for network communication
//!    - Provides Room access to NetworkActors

use actix::prelude::*;
use std::collections::HashMap;
use zznet_api::types::{PeerId, RoomId};
use zznet_room::actor::RoomActor;
use zznet_router::RouterActor;

use crate::actor::MemDBActor;
use crate::network_actor::MemDBNetworkActor;
use crate::network_messages::MemDBMessage;
use crate::permissions::MemDBPermissions;

/// Custom factory for MemDB that creates NetworkActors with proper wiring
///
/// Replaces StandardRoomFactory to follow the three-actor pattern.
/// Creates NetworkActor first, then RoomActor, no wiring needed (NetworkActor doesn't need room_actor reference).
pub struct MemDBRoomFactory {
    main_actor: Addr<MemDBActor>,
    manager: Addr<MemDBNetworkManager>,
    permissions_map: HashMap<String, MemDBPermissions>,
}

impl MemDBRoomFactory {
    /// Create a new MemDBRoomFactory
    ///
    /// # Arguments
    /// * `main_actor` - Address of the MainActor for business logic
    /// * `manager` - Address of the NetworkManager for registration
    /// * `permissions_map` - Map from role strings to permissions
    pub fn new(
        main_actor: Addr<MemDBActor>,
        manager: Addr<MemDBNetworkManager>,
        permissions_map: HashMap<String, MemDBPermissions>,
    ) -> Self {
        Self {
            main_actor,
            manager,
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
        let net = MemDBNetworkActor::new(
            peer_id.clone(),
            perms,
            self.main_actor.clone(),
            self.manager.clone(),
        );
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

/// NetworkManager orchestrates peer lifecycle for MemDB.
///
/// Pure lifecycle supervisor - no message routing
/// - Spawns/removes NetworkActors as peers join/leave
/// - NetworkActors handle their own request/reply directly
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
    /// Create a new NetworkManager.
    /// Pure lifecycle supervisor
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
        // Actors will be automatically stopped when dropped
    }
}

// ============================================================================
// Batch transmission handler
// ============================================================================
// When MainActor has a batch ready to send to Database peers,
// NetworkManager broadcasts it via RouterActor to all connected Database peers.

impl Handler<crate::internal_messages::BatchReadyToSend> for MemDBNetworkManager {
    type Result = ();

    fn handle(
        &mut self,
        msg: crate::internal_messages::BatchReadyToSend,
        _ctx: &mut Context<Self>,
    ) -> Self::Result {
        // Phase 9: Batch transmission - create SubmitBatch message for all Database peers
        // In the full implementation, this would broadcast via RouterActor to all connected peers
        let results_count = msg.results.len();
        let _network_msg = MemDBMessage::SubmitBatch {
            sender_peer_id: "collector".to_string(), // Will be filled by SessionManager
            timestamp_ms: msg.timestamp_ms,
            results: msg.results,
        };

        // TODO: Implement broadcast via RouterActor to send to all Database peers
        // For now, log that batch is ready to transmit
        tracing::info!(
            "Batch transmission prepared: timestamp={}, results_count={}",
            msg.timestamp_ms,
            results_count
        );
    }
}
