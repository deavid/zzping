//! Defines the wire format for the HELLO protocol.

use serde::{Deserialize, Serialize};

/// The top-level frame that distinguishes between handshake and data messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Frame {
    /// A frame used during the initial connection setup.
    Handshake(HandshakeFrame),
    /// A frame used for data exchange after the handshake is complete.
    Room(RoomFrame),
}

/// Frames exchanged exclusively during the handshake phase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum HandshakeFrame {
    /// The initial greeting, containing the peer's version, role, and hostname.
    Hello {
        version: String,
        role_str: String,
        hostname: String,
    },

    /// A proposal of rooms the peer wishes to communicate through.
    Offer { rooms: Vec<String> },

    /// A message indicating that the handshake has failed.
    Error { message: String },
}

/// Frames used for data exchange within a session after the handshake.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum RoomFrame {
    /// A message sent from a source room to a destination room.
    Message {
        from_room: String,
        to_room: String,
        payload: Vec<u8>,
    },

    /// A notification for a graceful disconnect.
    Disconnect,
}

impl Frame {
    /// Serializes the frame into a byte vector for transport.
    pub(crate) fn serialize(&self) -> Result<Vec<u8>, bincode::error::EncodeError> {
        bincode::serde::encode_to_vec(self, bincode::config::standard())
    }

    /// Deserializes a frame from a byte slice.
    pub(crate) fn deserialize(data: &[u8]) -> Result<Self, bincode::error::DecodeError> {
        bincode::serde::decode_from_slice(data, bincode::config::standard()).map(|(d, _)| d)
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
