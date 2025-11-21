//! Defines the wire format for the ZZNet protocol.
//!
//! This module contains the canonical definitions for all frame types used
//! in the ZZNet wire protocol. All components must use these definitions
//! to ensure protocol compatibility.

use serde::{Deserialize, Serialize};

/// The top-level frame that distinguishes between handshake and data messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Frame {
    /// A frame used during the initial connection setup.
    Handshake(HandshakeFrame),
    /// A frame used for data exchange after the handshake is complete.
    Room(RoomFrame),
}

/// Frames exchanged exclusively during the handshake phase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HandshakeFrame {
    /// The initial greeting, containing the peer's version, role, and hostname.
    Hello {
        /// Protocol version string
        version: String,
        /// Role identifier string from the peer
        role_str: String,
        /// Hostname of the peer
        hostname: String,
    },

    /// A proposal of rooms the peer wishes to communicate through.
    Offer {
        /// List of room names being offered
        rooms: Vec<String>,
    },

    /// A message indicating that the handshake has failed.
    Error {
        /// Human-readable error message
        message: String,
    },
}

/// Frames used for data exchange within a session after the handshake.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoomFrame {
    /// A message sent from a source room to a destination room.
    Message {
        /// Source room identifier
        from_room: String,
        /// Destination room identifier
        to_room: String,
        /// Binary message payload
        payload: Vec<u8>,
    },

    /// A notification for a graceful disconnect.
    Disconnect,
}

impl Frame {
    /// Serializes the frame into a byte vector for transport.
    pub fn serialize(&self) -> Result<Vec<u8>, rmp_serde::encode::Error> {
        rmp_serde::to_vec(self)
    }

    /// Deserializes a frame from a byte slice.
    pub fn deserialize(data: &[u8]) -> Result<Self, rmp_serde::decode::Error> {
        rmp_serde::from_slice(data)
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
