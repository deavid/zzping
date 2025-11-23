//! Component specification for MemDB.
//!
//! This module defines the `MemDBSpec` struct that implements `NetComponent`,
//! allowing the MemDB component to use the generic network machinery.

use crate::actor::MemDBActor;
use crate::events::MemDBEvent;
use crate::network_actor::MemDBNetworkActor;
use crate::network_messages::MemDBMessage;
use crate::permissions::MemDBPermissions;
use actix::prelude::*;

/// Component specification for MemDB.
///
/// This struct implements `NetComponent` to define all the types needed
/// by the generic network machinery to handle this component.
pub(crate) struct MemDBSpec;

impl zznet_component::NetComponent for MemDBSpec {
    const ROOM_ID: &'static str = "memdb";

    type MainActor = MemDBActor;
    type NetworkMsg = MemDBMessage;
    type Event = MemDBEvent;
    type Permissions = MemDBPermissions;
    type NetworkActor = MemDBNetworkActor;

    fn build_network_actor(
        peer_id: zznet_api::PeerId,
        permissions: Self::Permissions,
        main_actor: Addr<Self::MainActor>,
        event_rx: tokio::sync::broadcast::Receiver<Self::Event>,
        room_actor: Addr<zznet_room::RoomActor<Self::NetworkMsg>>,
    ) -> Self::NetworkActor {
        MemDBNetworkActor::new(peer_id, permissions, main_actor, event_rx, room_actor)
    }
}
