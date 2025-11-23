//! Component specification for ComponentA in zznet-demo.
//!
//! This module defines the spec that implements `NetComponent`,
//! allowing ComponentA to use the generic network machinery.

use crate::component_a::{ComponentAActor, ComponentANetworkActor, ComponentAPermissions};
use crate::messages::{ComponentAEvent, ComponentAMessage};
use actix::prelude::*;

/// Component specification for ComponentA.
pub(crate) struct ComponentASpec;

impl zznet_component::NetComponent for ComponentASpec {
    const ROOM_ID: &'static str = "room-a";

    type MainActor = ComponentAActor;
    type NetworkMsg = ComponentAMessage;
    type Event = ComponentAEvent;
    type Permissions = ComponentAPermissions;
    type NetworkActor = ComponentANetworkActor;

    fn build_network_actor(
        peer_id: zznet_api::PeerId,
        permissions: Self::Permissions,
        main_actor: Addr<Self::MainActor>,
        event_rx: tokio::sync::broadcast::Receiver<Self::Event>,
        room_actor: Addr<zznet_room::RoomActor<Self::NetworkMsg>>,
    ) -> Self::NetworkActor {
        ComponentANetworkActor::new(peer_id, permissions, main_actor, event_rx, room_actor)
    }
}
