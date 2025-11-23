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
