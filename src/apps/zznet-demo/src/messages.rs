//! Message types for zznet-demo components.
//!
//! This module defines the network and local messages used by the demo components.

use actix::{Addr, Message, Recipient};
use serde::{Deserialize, Serialize};
use zznet_api::RoomId;
use zznet_room::{DeserializationError, RoomMessageTrait, SerializationError};

/// Enum for messages handled by ComponentA's RoomActor.
/// These messages are sent over the network.
#[derive(Serialize, Deserialize, Message, Debug, Clone)]
#[rtype(result = "()")]
pub enum ComponentAMessage {
    /// A ping message with a counter and some data.
    Ping((u64, String)),
    /// A pong message with a counter and some data.
    Pong((u64, String)),
}

impl RoomMessageTrait for ComponentAMessage {
    fn room_id(&self) -> RoomId {
        "room-a".into()
    }

    fn serialize_inner(&self) -> Result<Vec<u8>, SerializationError> {
        rmp_serde::to_vec(self).map_err(|e| SerializationError::MsgPackError(e.to_string()))
    }

    fn deserialize_for_room(room_id: &RoomId, bytes: &[u8]) -> Result<Self, DeserializationError> {
        if room_id.as_str() != "room-a" {
            return Err(DeserializationError::UnknownRoom(room_id.clone()));
        }
        let msg = rmp_serde::from_slice(bytes)
            .map_err(|e| DeserializationError::MsgPackError(e.to_string()))?;
        Ok(msg)
    }
}

/// A message to instruct ComponentA to send a ping.
/// This is used for testing purposes.
#[derive(Message, Clone)]
#[rtype(result = "()")]
pub struct SendPing {
    /// The data to include in the ping.
    pub data: String,
}

/// A message to instruct ComponentA to publish its state.
#[derive(Message, Clone)]
#[rtype(result = "()")]
pub struct PublishToA {
    /// The data to include in the state publication.
    pub data: String,
}

/// A message from ComponentB to ComponentA to initiate a ping.
#[derive(Message, Clone)]
#[rtype(result = "()")]
pub struct SendPingFromB {
    /// The data to include in the ping.
    pub data: String,
}

/// A message representing a state update.
#[derive(Message, Clone)]
#[rtype(result = "()")]
pub struct StateUpdate {
    /// The new counter value.
    pub counter: u64,
    /// The new data value.
    pub data: String,
}

/// Event for ComponentA state changes
#[derive(Clone, Debug, Message)]
#[rtype(result = "()")]
pub enum ComponentAEvent {
    /// State changed with new counter and data
    StateChanged {
        /// The new counter value
        counter: u64,
        /// The new data value
        data: String,
    },
    /// Pong sent
    Pong {
        /// The counter value
        counter: u64,
        /// The data value
        data: String,
    },
}

/// A message to get the current counter value from a component.
#[derive(Message, Clone)]
#[rtype(result = "u64")]
pub struct GetCounter;

/// A message to get the event bus sender.
#[derive(Message, Clone)]
#[rtype(result = "tokio::sync::broadcast::Sender<ComponentAEvent>")]
pub struct GetEventBus;

/// A message to subscribe to ComponentA's state updates.
#[derive(Message, Clone)]
#[rtype(result = "()")]
pub struct Subscribe {
    /// The recipient to send state updates to.
    pub recipient: Recipient<StateUpdate>,
}

/// A message to set the address of ComponentA in ComponentB.
#[derive(Message, Clone)]
#[rtype(result = "()")]
pub struct SetComponentA {
    /// The address of the ComponentA actor.
    pub component_a: Addr<crate::component_a::ComponentAActor>,
}
