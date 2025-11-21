//! Generic messages for NetworkComponent pattern.
//!
//! This module provides generic message types that work with the NetworkComponent trait,
//! eliminating the need for each component to define its own message types.

use crate::factory_utils::NetworkComponent;
use actix::prelude::*;

/// Generic RegisterPeer message for registering a peer with a NetworkManager.
///
/// This message is sent fire-and-forget from the RoomFactory to the NetworkManager
/// after the peer's room and network actors are created. The factory creates the
/// actors synchronously, and this message allows the manager to track them.
///
/// Generic over `C: NetworkComponent` to ensure type safety - the message carries
/// addresses that are guaranteed to be compatible with the component's types.
#[derive(Message)]
#[rtype(result = "()")]
pub struct RegisterPeer<C: NetworkComponent> {
    /// The peer ID
    pub peer_id: zznet_api::types::PeerId,
    /// The NetworkActor for this peer
    pub network_actor: Addr<C::NetworkActor>,
    /// The RoomActor for this peer
    pub room_actor: Addr<zznet_room::actor::RoomActor<C::ProtocolMessage>>,
}
