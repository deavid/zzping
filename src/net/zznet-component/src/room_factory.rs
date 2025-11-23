//! Generic RoomFactory implementation.

use crate::messages::SetRoomActor;
use crate::traits::NetComponent;
use actix::prelude::*;
use std::collections::HashMap;
use zznet_api::{PeerId, RoomId};
use zznet_room::RoomActor;

/// Generic factory for creating rooms for network components.
///
/// This factory is parameterized by a `NetComponent` implementation and uses
/// the types defined in that component to automatically create and wire the
/// NetworkActor and RoomActor.
///
/// # Type Parameter
/// - `C`: The component specification implementing `NetComponent`
pub struct GenericRoomFactory<C: NetComponent> {
    /// Address of the main business logic actor
    main_actor: Addr<C::MainActor>,

    /// Event bus sender for broadcasting component events
    event_bus: tokio::sync::broadcast::Sender<C::Event>,

    /// Map from role strings to permissions
    permissions_map: HashMap<String, C::Permissions>,
}

impl<C: NetComponent> GenericRoomFactory<C> {
    /// Create a new GenericRoomFactory.
    ///
    /// # Arguments
    /// - `main_actor`: Address of the MainActor for this component
    /// - `event_bus`: Event bus sender for broadcasting state changes
    /// - `permissions_map`: Map from role strings to permissions
    pub fn new(
        main_actor: Addr<C::MainActor>,
        event_bus: tokio::sync::broadcast::Sender<C::Event>,
        permissions_map: HashMap<String, C::Permissions>,
    ) -> Self {
        Self {
            main_actor,
            event_bus,
            permissions_map,
        }
    }
}

impl<C: NetComponent> zznet_router::RoomFactory for GenericRoomFactory<C> {
    fn create_room(
        &self,
        peer_id: PeerId,
        role: zznet_api::Role,
        room_id: RoomId,
        transport_tx: tokio::sync::mpsc::Sender<zznet_api::TransportFrame>,
    ) -> Result<Option<zznet_room::RoomInboundRecipient>, String> {
        // Check if this is our room
        if room_id.as_str() != C::ROOM_ID {
            return Ok(None);
        }

        log::debug!(
            "Creating room '{}' for peer {} with role {}",
            C::ROOM_ID,
            peer_id,
            role.as_str()
        );

        // Lookup permissions
        let perms = self
            .permissions_map
            .get(role.as_str())
            .cloned()
            .unwrap_or_default();

        // Create NetworkActor without room_actor (resolves circular dependency)
        let net = C::build_network_actor(
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
            net_addr.clone().recipient::<C::NetworkMsg>(),
        );
        let room_addr = room.start();

        // Wire them together via SetRoomActor message
        net_addr.do_send(SetRoomActor(room_addr.clone()));

        Ok(Some(room_addr.recipient()))
    }
}
