//! Defines the public message bus interface for the ZzNetConnManager.
//!
//! This module contains the message types that external components (like a
//! ZzNetRoomManager) use to subscribe to and receive notifications about the

//! availability of logical "Rooms" on new network connections.

use crate::actor::ZzNetConnActor;
use actix::prelude::*;

// A type alias for clarity. This is the handle that allows a RoomManager
// to interact with the specific connection where its room is active.
pub type ConnectionActorHandle = Addr<ZzNetConnActor>;

/// A message sent by a RoomManager to the ZzNetConnManager to register
/// interest in a specific room.
#[derive(Message, Clone)]
#[rtype(result = "()")]
pub struct SubscribeToRoom {
    /// The name of the logical room (e.g., "intent-config").
    pub room_name: String,
    /// The address of the subscriber, which must be able to handle
    /// `RoomIsActive` messages.
    pub subscriber: Recipient<RoomIsActive>,
}

/// A message sent by a RoomManager to the ZzNetConnManager to cancel
/// its interest in a specific room.
#[derive(Message, Clone)]
#[rtype(result = "()")]
pub struct UnsubscribeFromRoom {
    /// The name of the logical room.
    pub room_name: String,
    // In a real system, we might need a way to identify which subscriber
    // is unsubscribing if multiple can subscribe to the same room. For now,
    // we'll assume one subscriber per room name.
}

/// A notification message sent from the ZzNetConnManager to a subscriber
/// when a new connection has been established and the requested room is active.
#[derive(Message, Clone)]
#[rtype(result = "()")]
pub struct RoomIsActive {
    /// The name of the room that is now active.
    pub room_name: String,
    /// A handle to the specific `ZzNetConnActor` managing the connection
    /// where this room is active. The subscriber will use this handle to
    /// send and receive data for the room.
    pub connection_actor: ConnectionActorHandle,
    // A unique identifier for the underlying connection could be added here
    // if the subscriber needs to distinguish between multiple connections.
    // pub connection_id: u64,
}
