//! Component specification for IntentConfig.
//!
//! This module defines the `IntentConfigSpec` struct that implements `NetComponent`,
//! allowing the IntentConfig component to use the generic network machinery.

use crate::actor::IntentConfigActor;
use crate::events::IntentConfigEvent;
use crate::network_actor::IntentConfigNetworkActor;
use crate::network_messages::IntentConfigNetworkMsg;
use crate::permissions::IntentConfigPermissions;
use actix::prelude::*;

/// Component specification for IntentConfig.
///
/// This struct implements `NetComponent` to define all the types needed
/// by the generic network machinery to handle this component.
pub(crate) struct IntentConfigSpec;

impl zznet_component::NetComponent for IntentConfigSpec {
    const ROOM_ID: &'static str = "intent-config";

    type MainActor = IntentConfigActor;
    type NetworkMsg = IntentConfigNetworkMsg;
    type Event = IntentConfigEvent;
    type Permissions = IntentConfigPermissions;
    type NetworkActor = IntentConfigNetworkActor;

    fn build_network_actor(
        peer_id: zznet_api::PeerId,
        permissions: Self::Permissions,
        main_actor: Addr<Self::MainActor>,
        event_rx: tokio::sync::broadcast::Receiver<Self::Event>,
        room_actor: Addr<zznet_room::RoomActor<Self::NetworkMsg>>,
    ) -> Self::NetworkActor {
        IntentConfigNetworkActor::new(peer_id, permissions, main_actor, event_rx, room_actor)
    }
}
