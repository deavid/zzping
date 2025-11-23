//! Component specification for CState.
//!
//! This module defines the `CStateSpec` struct that implements `NetComponent`,
//! allowing the CState component to use the generic network machinery.

use crate::actor::CStateActor;
use crate::events::CStateEvent;
use crate::network_actor::CStateNetworkActor;
use crate::network_messages::CStateMessage;
use crate::permissions::CStatePermissions;
use actix::prelude::*;

/// Component specification for CState.
///
/// This struct implements `NetComponent` to define all the types needed
/// by the generic network machinery to handle this component.
pub(crate) struct CStateSpec;

impl zznet_component::NetComponent for CStateSpec {
    const ROOM_ID: &'static str = "cstate";

    type MainActor = CStateActor;
    type NetworkMsg = CStateMessage;
    type Event = CStateEvent;
    type Permissions = CStatePermissions;
    type NetworkActor = CStateNetworkActor;

    fn build_network_actor(
        peer_id: zznet_api::PeerId,
        permissions: Self::Permissions,
        main_actor: Addr<Self::MainActor>,
        event_rx: tokio::sync::broadcast::Receiver<Self::Event>,
        room_actor: Addr<zznet_room::RoomActor<Self::NetworkMsg>>,
    ) -> Self::NetworkActor {
        CStateNetworkActor::new(peer_id, permissions, main_actor, event_rx, room_actor)
    }
}
