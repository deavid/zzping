//! Protocol frame definitions for HELLO protocol.
//!
//! This module defines the wire format for handshake and room communication.
//! Frames are serialized using bincode and sent over the transport layer.

use serde::{Deserialize, Serialize};

/// Top-level frame enum.
///
/// This distinguishes between handshake frames (used during connection setup)
/// and room frames (used after handshake completes).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Frame {
    /// Handshake frame - used during connection establishment.
    Handshake(HandshakeFrame),
    /// Room frame - used for actual room-to-room communication.
    Room(RoomFrame),
}

/// Frames exchanged during the handshake phase.
///
/// The handshake follows this sequence:
/// 1. Initiator sends HELLO
/// 2. Responder sends HELLO
/// 3. Both sides send OFFER with their available rooms
/// 4. Both sides send ACK with selected rooms
/// 5. Handshake complete
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HandshakeFrame {
    /// Initial greeting with protocol version, role, and hostname.
    Hello {
        /// Protocol version (currently "1.0").
        version: String,
        /// The role identifier of this peer as a string (application CN).
        role_str: String,
        /// Hostname/identifier of this peer.
        hostname: String,
    },

    /// Offer available rooms to the peer.
    ///
    /// Each side sends the list of rooms they can communicate through.
    Offer {
        /// List of room names this peer offers.
        rooms: Vec<String>,
    },

    /// Acknowledge handshake with selected rooms.
    ///
    /// The selected rooms are the intersection of both peers' offered rooms,
    /// filtered by authorization rules.
    Ack {
        /// List of room names both peers will use.
        rooms: Vec<String>,
    },

    /// Handshake error - peer is rejecting the connection.
    Error {
        /// Human-readable error message.
        message: String,
    },
}

/// Frames used after handshake for room communication.
///
/// These frames carry the actual application messages between rooms.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoomFrame {
    /// A message from one room to another.
    Message {
        /// Source room name.
        from_room: String,
        /// Destination room name.
        to_room: String,
        /// Serialized message payload.
        payload: Vec<u8>,
    },

    /// Graceful disconnect notification.
    Disconnect,
}

impl Frame {
    /// Serialize this frame to bytes.
    ///
    /// Returns the serialized bytes suitable for sending over a transport.
    pub fn serialize(&self) -> Result<Vec<u8>, bincode::error::EncodeError> {
        bincode::serde::encode_to_vec(self, bincode::config::standard())
    }

    /// Deserialize a frame from bytes.
    ///
    /// Returns the deserialized frame or an error if the data is malformed.
    pub fn deserialize(data: &[u8]) -> Result<Self, bincode::error::DecodeError> {
        let (d, _) = bincode::serde::decode_from_slice(data, bincode::config::standard())?;
        Ok(d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hello_frame_roundtrip() {
        let frame = Frame::Handshake(HandshakeFrame::Hello {
            version: "1.0".to_string(),
            role_str: "collector".to_string(),
            hostname: "test-host".to_string(),
        });

        let bytes = frame.serialize().unwrap();
        let decoded = Frame::deserialize(&bytes).unwrap();

        assert_eq!(frame, decoded);
    }

    #[test]
    fn test_offer_frame_roundtrip() {
        let frame = Frame::Handshake(HandshakeFrame::Offer {
            rooms: vec!["memdb".to_string(), "query".to_string()],
        });

        let bytes = frame.serialize().unwrap();
        let decoded = Frame::deserialize(&bytes).unwrap();

        assert_eq!(frame, decoded);
    }

    #[test]
    fn test_ack_frame_roundtrip() {
        let frame = Frame::Handshake(HandshakeFrame::Ack {
            rooms: vec!["memdb".to_string()],
        });

        let bytes = frame.serialize().unwrap();
        let decoded = Frame::deserialize(&bytes).unwrap();

        assert_eq!(frame, decoded);
    }

    #[test]
    fn test_error_frame_roundtrip() {
        let frame = Frame::Handshake(HandshakeFrame::Error {
            message: "Version mismatch".to_string(),
        });

        let bytes = frame.serialize().unwrap();
        let decoded = Frame::deserialize(&bytes).unwrap();

        assert_eq!(frame, decoded);
    }

    #[test]
    fn test_room_message_frame_roundtrip() {
        let frame = Frame::Room(RoomFrame::Message {
            from_room: "collector".to_string(),
            to_room: "memdb".to_string(),
            payload: vec![1, 2, 3, 4, 5],
        });

        let bytes = frame.serialize().unwrap();
        let decoded = Frame::deserialize(&bytes).unwrap();

        assert_eq!(frame, decoded);
    }

    #[test]
    fn test_disconnect_frame_roundtrip() {
        let frame = Frame::Room(RoomFrame::Disconnect);

        let bytes = frame.serialize().unwrap();
        let decoded = Frame::deserialize(&bytes).unwrap();

        assert_eq!(frame, decoded);
    }

    #[test]
    fn test_invalid_data_deserialization() {
        let invalid_data = vec![0xFF, 0xFF, 0xFF, 0xFF];
        let result = Frame::deserialize(&invalid_data);

        assert!(result.is_err());
    }

    #[test]
    fn test_all_roles_serialize() {
        for role_str in &["collector", "database", "client-ro", "client-admin"] {
            let frame = Frame::Handshake(HandshakeFrame::Hello {
                version: "1.0".to_string(),
                role_str: role_str.to_string(),
                hostname: "test-host".to_string(),
            });

            let bytes = frame.serialize().unwrap();
            let decoded = Frame::deserialize(&bytes).unwrap();

            assert_eq!(frame, decoded);
        }
    }
}
