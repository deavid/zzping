//! Standard messages used in the generic component system.

use actix::prelude::*;
use zznet_room::RoomActor;

/// Standard message to provide the RoomActor address to the NetworkActor.
///
/// This message is used to wire the NetworkActor to the RoomActor after both
/// have been created. This resolves the circular dependency in the factory.
///
/// # Type Parameter
/// - `T`: The specific Network Message type (e.g., IntentConfigNetworkMsg)
///   that implements RoomMessageTrait.
#[derive(Clone)]
pub struct SetRoomActor<T>(pub Addr<RoomActor<T>>)
where
    T: zznet_room::RoomMessageTrait + Message<Result = ()> + Send;

impl<T> Message for SetRoomActor<T>
where
    T: zznet_room::RoomMessageTrait + Message<Result = ()> + Send,
{
    type Result = ();
}
