//! Serialization bridge between typed messages and protocol frames.
//!
//! This module provides helpers to convert:
//! - Typed room messages → RoomFrame (bytes)
//! - RoomFrame (bytes) → Typed room messages
//!
//! This is the boundary where application-level typed messages cross into
//! the wire protocol.

use crate::error::HelloError;
use crate::protocol::{Frame, RoomFrame};
use serde::{Deserialize, Serialize};

/// Serialize a typed message for sending through a room.
///
/// This takes any serializable message, converts it to bytes using bincode,
/// and wraps it in a RoomFrame ready for transmission.
///
/// # Arguments
/// * `from_room` - Source room name
/// * `to_room` - Destination room name
/// * `message` - The typed message to serialize
///
/// # Returns
/// Serialized Frame ready to send over transport
pub fn serialize_room_message<T: Serialize>(
    from_room: &str,
    to_room: &str,
    message: &T,
) -> Result<Vec<u8>, HelloError> {
    // Serialize the message payload
    let payload =
        bincode::serialize(message).map_err(|e| HelloError::Serialization(e.to_string()))?;

    // Wrap in RoomFrame
    let room_frame = RoomFrame::Message {
        from_room: from_room.to_string(),
        to_room: to_room.to_string(),
        payload,
    };

    // Wrap in Frame and serialize
    let frame = Frame::Room(room_frame);
    frame.serialize().map_err(|e| e.into())
}

/// Deserialize a typed message from a RoomFrame.
///
/// This extracts the payload from a RoomFrame and deserializes it into
/// the expected typed message.
///
/// # Arguments
/// * `frame_data` - Raw frame bytes received from transport
///
/// # Returns
/// Tuple of (from_room, to_room, deserialized_message)
pub fn deserialize_room_message<T: for<'de> Deserialize<'de>>(
    frame_data: &[u8],
) -> Result<(String, String, T), HelloError> {
    // Deserialize the frame
    let frame = Frame::deserialize(frame_data)?;

    // Extract RoomFrame
    match frame {
        Frame::Room(RoomFrame::Message {
            from_room,
            to_room,
            payload,
        }) => {
            // Deserialize the payload
            let message: T = bincode::deserialize(&payload)
                .map_err(|e| HelloError::Serialization(e.to_string()))?;

            Ok((from_room, to_room, message))
        }
        Frame::Room(RoomFrame::Disconnect) => Err(HelloError::InvalidState(
            "Received Disconnect frame".to_string(),
        )),
        Frame::Handshake(_) => Err(HelloError::InvalidState(
            "Received Handshake frame when expecting Room frame".to_string(),
        )),
    }
}

/// Create a Disconnect frame for graceful connection termination.
pub fn create_disconnect_frame() -> Result<Vec<u8>, HelloError> {
    let frame = Frame::Room(RoomFrame::Disconnect);
    frame.serialize().map_err(|e| e.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct TestMessage {
        id: u64,
        data: String,
    }

    #[test]
    fn test_serialize_deserialize_roundtrip() {
        let msg = TestMessage {
            id: 42,
            data: "test data".to_string(),
        };

        // Serialize
        let frame_data = serialize_room_message("memdb", "query", &msg).unwrap();

        // Deserialize
        let (from_room, to_room, decoded_msg): (String, String, TestMessage) =
            deserialize_room_message(&frame_data).unwrap();

        assert_eq!(from_room, "memdb");
        assert_eq!(to_room, "query");
        assert_eq!(decoded_msg, msg);
    }

    #[test]
    fn test_serialize_different_types() {
        // Test with simple types
        let num_frame = serialize_room_message("room1", "room2", &123u64).unwrap();
        let (_from, _to, num): (String, String, u64) =
            deserialize_room_message(&num_frame).unwrap();
        assert_eq!(num, 123);

        let str_frame = serialize_room_message("room1", "room2", &"hello".to_string()).unwrap();
        let (_from, _to, s): (String, String, String) =
            deserialize_room_message(&str_frame).unwrap();
        assert_eq!(s, "hello");
    }

    #[test]
    fn test_deserialize_disconnect_frame() {
        let frame_data = create_disconnect_frame().unwrap();

        // Should fail when trying to deserialize as message
        let result: Result<(String, String, TestMessage), HelloError> =
            deserialize_room_message(&frame_data);

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), HelloError::InvalidState(_)));
    }

    #[test]
    fn test_deserialize_handshake_frame_fails() {
        use crate::protocol::HandshakeFrame;

        let handshake_frame = Frame::Handshake(HandshakeFrame::Hello {
            version: "1.0".to_string(),
            role: crate::auth::AuthRole::Collector,
            hostname: "test-host".to_string(),
        });
        let frame_data = handshake_frame.serialize().unwrap();

        // Should fail when trying to deserialize as room message
        let result: Result<(String, String, TestMessage), HelloError> =
            deserialize_room_message(&frame_data);

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), HelloError::InvalidState(_)));
    }

    #[test]
    fn test_invalid_frame_data() {
        // Test that deserializing garbage data fails gracefully
        let garbage = vec![0xFF, 0xFF, 0xFF, 0xFF];
        let result: Result<(String, String, TestMessage), HelloError> =
            deserialize_room_message(&garbage);

        assert!(result.is_err());
    }

    #[test]
    fn test_create_disconnect_frame() {
        let frame_data = create_disconnect_frame().unwrap();

        // Verify it deserializes to a Disconnect frame
        let frame = Frame::deserialize(&frame_data).unwrap();
        assert!(matches!(frame, Frame::Room(RoomFrame::Disconnect)));
    }

    #[test]
    fn test_large_message_serialization() {
        let large_msg = TestMessage {
            id: 1,
            data: "x".repeat(10000), // 10KB of data
        };

        let frame_data = serialize_room_message("room1", "room2", &large_msg).unwrap();
        let (_from, _to, decoded): (String, String, TestMessage) =
            deserialize_room_message(&frame_data).unwrap();

        assert_eq!(decoded, large_msg);
    }

    #[test]
    fn test_empty_room_names() {
        let msg = TestMessage {
            id: 1,
            data: "test".to_string(),
        };

        let frame_data = serialize_room_message("", "", &msg).unwrap();
        let (from_room, to_room, _decoded): (String, String, TestMessage) =
            deserialize_room_message(&frame_data).unwrap();

        assert_eq!(from_room, "");
        assert_eq!(to_room, "");
    }
}
