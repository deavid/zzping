//! Generic RoomFactory implementation.

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

        // Clone data needed for the closure
        let peer_id_clone = peer_id.clone();
        let main_actor_clone = self.main_actor.clone();
        let event_rx = self.event_bus.subscribe();
        let perms_clone = perms;

        // Setup channel to retrieve RoomActor recipient from inside the closure
        let (tx, rx) = std::sync::mpsc::channel();

        // Create NetworkActor using Actor::create to access context before construction
        let _net_addr = C::NetworkActor::create(move |ctx| {
            // Get NetworkActor's address immediately from context
            let net_recipient = ctx.address().recipient();

            // Create and start RoomActor, wiring it to the NetworkActor
            let room_actor = RoomActor::new(room_id, transport_tx, net_recipient).start();

            // Send RoomActor's recipient back to the factory
            let _ = tx.send(room_actor.clone().recipient());

            // Construct NetworkActor with room_actor already wired
            C::build_network_actor(
                peer_id_clone,
                perms_clone,
                main_actor_clone,
                event_rx,
                room_actor,
            )
        });

        // Retrieve the room recipient to return to the Router
        let room_recipient = rx
            .recv()
            .map_err(|e| format!("Failed to create RoomActor: {}", e))?;

        Ok(Some(room_recipient))
    }
}
