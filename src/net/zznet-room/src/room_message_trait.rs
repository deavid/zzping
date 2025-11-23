//! Room message trait and related types
//!
//! This module defines the trait that application-defined message enums must implement
//! to work with the network framework.

use std::error::Error as StdError;
use std::fmt;
use zznet_api::RoomId;

/// Error during message serialization
#[derive(Debug)]
pub enum SerializationError {
    /// Serialization failed
    Failed(String),
    /// MessagePack serialization error
    MsgPackError(String),
}

impl fmt::Display for SerializationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SerializationError::Failed(msg) => write!(f, "Serialization failed: {}", msg),
            SerializationError::MsgPackError(msg) => write!(f, "MessagePack error: {}", msg),
        }
    }
}

impl StdError for SerializationError {}

/// Error during message deserialization
#[derive(Debug)]
pub enum DeserializationError {
    /// Unknown room ID (not in application's enum)
    UnknownRoom(RoomId),
    /// Deserialization failed
    Failed(String),
    /// MessagePack deserialization error
    MsgPackError(String),
    /// Custom error (for test convenience)
    Custom(String),
}

impl fmt::Display for DeserializationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeserializationError::UnknownRoom(room_id) => {
                write!(f, "Unknown room: {}", room_id)
            }
            DeserializationError::Failed(msg) => {
                write!(f, "Deserialization failed: {}", msg)
            }
            DeserializationError::MsgPackError(msg) => {
                write!(f, "MessagePack error: {}", msg)
            }
            DeserializationError::Custom(msg) => {
                write!(f, "Custom error: {}", msg)
            }
        }
    }
}

impl StdError for DeserializationError {}

/// Trait that application-defined message enums must implement
///
/// This trait enables the network framework to work with any application's
/// message enum while maintaining type safety and allowing different applications to
/// have different enum definitions.
pub trait RoomMessageTrait: Clone + Send + Sync + Unpin + std::fmt::Debug + 'static {
    /// Get the room ID for this message
    ///
    /// The room ID is determined by which enum variant this is.
    /// Each variant corresponds to one room.
    fn room_id(&self) -> RoomId;

    /// Serialize just the inner message (not the enum wrapper)
    ///
    /// This serializes the concrete message type wrapped by the enum variant,
    /// NOT the enum structure itself. Over the wire, we send:
    /// `(RoomId, serialized_inner_message)` not `serialized_enum`.
    fn serialize_inner(&self) -> Result<Vec<u8>, SerializationError>;

    /// Deserialize from room ID + bytes
    ///
    /// Given a room ID and serialized bytes, reconstruct the appropriate enum variant.
    /// The room ID determines which variant to create, and the bytes are deserialized
    /// into the inner message type.
    fn deserialize_for_room(room_id: &RoomId, bytes: &[u8]) -> Result<Self, DeserializationError>;
}
