//! Message types for zznet-demo components.
//!
//! This module defines the network and local messages used by the demo components.

use actix::{Addr, Message, Recipient};
use serde::{Deserialize, Serialize};
use zznet_api::types::RoomId;
use zznet_room::room_message_trait::{DeserializationError, RoomMessageTrait, SerializationError};

use crate::component_a::ComponentANetworkManager;

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
        bincode::serde::encode_to_vec(self, bincode::config::standard())
            .map_err(|e| SerializationError::BincodeError(e.to_string()))
    }

    fn deserialize_for_room(room_id: &RoomId, bytes: &[u8]) -> Result<Self, DeserializationError> {
        if room_id.as_str() != "room-a" {
            return Err(DeserializationError::UnknownRoom(room_id.clone()));
        }
        let (msg, _) = bincode::serde::decode_from_slice(bytes, bincode::config::standard())
            .map_err(|e| DeserializationError::BincodeError(e.to_string()))?;
        Ok(msg)
    }

    fn supported_rooms() -> Vec<RoomId> {
        vec!["room-a".into()]
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

/// A message to get the current counter value from a component.
#[derive(Message, Clone)]
#[rtype(result = "u64")]
pub struct GetCounter;

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

/// A message to set the address of the NetworkManager in ComponentA.
#[derive(Message, Clone)]
#[rtype(result = "()")]
pub struct SetNetworkManager {
    /// The address of the ComponentANetworkManager actor.
    pub network_manager: Addr<ComponentANetworkManager>,
}
