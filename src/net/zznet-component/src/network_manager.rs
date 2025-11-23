//! Generic NetworkManager implementation.

use crate::room_factory::GenericRoomFactory;
use crate::traits::NetComponent;
use actix::prelude::*;
use std::collections::HashMap;
use zznet_api::RoomId;
use zznet_router::RouterActor;

/// Generic NetworkManager that handles peer lifecycle for any component.
///
/// This actor orchestrates the per-peer translator layer using the types
/// defined in the `NetComponent` trait. It registers with the router and
/// uses a `GenericRoomFactory` to create rooms and wire actors together.
///
/// # Type Parameter
/// - `C`: The component specification implementing `NetComponent`
///
/// # Responsibilities
/// - Register with the RouterActor for the component's room
/// - Use GenericRoomFactory to handle peer connections
/// - Stay alive to keep the factory registered
pub struct GenericNetworkManager<C: NetComponent> {
    /// Address of the main business logic actor
    main_actor: Addr<C::MainActor>,

    /// Event bus for broadcasting component events to all NetworkActors
    event_bus: tokio::sync::broadcast::Sender<C::Event>,

    /// RouterActor for data-plane message routing
    router_actor: Addr<RouterActor>,

    /// Policy map from role strings to component-specific permissions
    permissions_map: HashMap<String, C::Permissions>,
}

impl<C: NetComponent> GenericNetworkManager<C> {
    /// Create a new GenericNetworkManager.
    ///
    /// # Arguments
    /// - `main_actor`: Address of the MainActor for this component
    /// - `router_actor`: Address of the RouterActor for registration
    /// - `event_bus`: Event bus sender for broadcasting state changes
    /// - `permissions_map`: Map from role strings to permissions
    pub fn new(
        main_actor: Addr<C::MainActor>,
        router_actor: Addr<RouterActor>,
        event_bus: tokio::sync::broadcast::Sender<C::Event>,
        permissions_map: HashMap<String, C::Permissions>,
    ) -> Self {
        Self {
            main_actor,
            event_bus,
            router_actor,
            permissions_map,
        }
    }
}

impl<C: NetComponent> Actor for GenericNetworkManager<C> {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        log::info!("GenericNetworkManager started for room '{}'", C::ROOM_ID);

        // Register with router using generic factory
        let factory = std::sync::Arc::new(GenericRoomFactory::<C>::new(
            self.main_actor.clone(),
            self.event_bus.clone(),
            self.permissions_map.clone(),
        ));
        let rooms = vec![RoomId::from(C::ROOM_ID)];
        let register_msg = zznet_router::RegisterManager { factory, rooms };
        self.router_actor.do_send(register_msg);

        // Keep actor alive
        ctx.set_mailbox_capacity(1000);
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        log::info!("GenericNetworkManager stopped for room '{}'", C::ROOM_ID);

        // Actors will be automatically stopped when dropped
    }
}
