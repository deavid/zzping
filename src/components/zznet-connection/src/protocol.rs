//! Contains the pure state machine logic for the zznet symmetric handshake protocol.
//!
//! This module is deliberately isolated from any actor or network I/O details,
//! allowing the handshake logic to be unit-tested in a simple, synchronous manner.

use crate::auth::AuthRole;
use crate::error::{Result, ZzNetConnectionError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum HandshakeFrame {
    Hello {
        protocol_version: String,
        auth_role: AuthRole,
        offered_rooms: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RoomFrame {
    PublishRooms { offered_rooms: Vec<String> },
    MessageForRoom { room: String, data: Vec<u8> },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Frame {
    Handshake(HandshakeFrame),
    Room(RoomFrame),
}

pub fn serialize(frame: &Frame) -> Result<Vec<u8>> {
    bincode::serialize(frame).map_err(ZzNetConnectionError::Serialization)
}

pub fn deserialize(data: &[u8]) -> Result<Frame> {
    bincode::deserialize(data).map_err(ZzNetConnectionError::Serialization)
}

/// Represents the current state of the handshake process.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum HandshakeState {
    /// The initial state before any messages have been sent or received.
    Start,
    /// The state after this peer has sent its `Hello` message but before it
    /// has received a `Hello` from the remote peer.
    SentHello { our_offered_rooms: Vec<String> },
    /// The final state, reached after both peers have exchanged `Hello` messages.
    /// Contains the final, negotiated set of active rooms.
    Complete { active_rooms: Vec<String> },
    /// A terminal state indicating the handshake has failed.
    Failed(String),
}

/// A state machine that manages the symmetric handshake protocol.
#[derive(Debug, Clone)]
pub struct Handshake {
    state: HandshakeState,
}

impl Handshake {
    /// Creates a new `Handshake` instance in the `Start` state.
    pub fn new() -> Self {
        Self {
            state: HandshakeState::Start,
        }
    }

    /// Checks if the handshake has successfully completed.
    pub fn is_complete(&self) -> bool {
        matches!(self.state, HandshakeState::Complete { .. })
    }

    /// If the handshake is complete, returns the list of negotiated active rooms.
    pub fn active_rooms(&self) -> Option<&[String]> {
        if let HandshakeState::Complete { active_rooms } = &self.state {
            Some(active_rooms)
        } else {
            None
        }
    }

    /// Creates the initial `Hello` frame to be sent to the remote peer.
    /// This action transitions the state machine to `SentHello`.
    pub fn create_hello_frame(
        &mut self,
        protocol_version: String,
        auth_role: AuthRole,
        offered_rooms: Vec<String>,
    ) -> Result<Vec<u8>> {
        match self.state {
            HandshakeState::Start => {
                let hello = HandshakeFrame::Hello {
                    protocol_version,
                    auth_role,
                    offered_rooms: offered_rooms.clone(),
                };
                let frame = Frame::Handshake(hello);
                let serialized = serialize(&frame)?;
                self.state = HandshakeState::SentHello {
                    our_offered_rooms: offered_rooms,
                };
                log::info!("Handshake state -> SentHello");
                Ok(serialized)
            }
            _ => Err(ZzNetConnectionError::InvalidState(format!(
                "Cannot create hello frame from state {:?}",
                self.state
            ))),
        }
    }

    /// Processes an incoming frame from the remote peer.
    ///
    /// This is the core of the state machine. It may change the internal state
    /// and may return a response frame that the calling actor must send.
    pub fn process_frame(&mut self, frame_data: Vec<u8>) -> Result<Option<Vec<u8>>> {
        let frame = deserialize(&frame_data)?;
        match frame {
            Frame::Handshake(HandshakeFrame::Hello {
                protocol_version: _,
                auth_role,
                offered_rooms: their_offered_rooms,
            }) => {
                match &self.state {
                    HandshakeState::Start => {
                        // We received a Hello before we sent ours. This is valid in a
                        // symmetric protocol. We should reply with our own Hello.
                        log::info!("Handshake: Received Hello while in Start state. Replying.");
                        // For now, we assume create_hello_frame is called first, so error.
                        self.state = HandshakeState::Failed(
                            "Received Hello before own rooms were configured".to_string(),
                        );
                        Err(ZzNetConnectionError::HandshakeFailed(
                            "Received Hello before create_hello_frame was called.".to_string(),
                        ))
                    }
                    HandshakeState::SentHello { our_offered_rooms } => {
                        // Validate that peer's auth_role can access offered rooms
                        let valid_rooms: Vec<String> = their_offered_rooms
                            .into_iter()
                            .filter(|room| auth_role.can_access_room(room))
                            .collect();

                        // This is the ideal case. We sent Hello, they sent Hello.
                        // Now we can compute the intersection and complete the handshake.
                        log::info!("Handshake: Received peer's Hello. Computing intersection.");
                        let intersection: Vec<String> = our_offered_rooms
                            .iter()
                            .filter(|&room| valid_rooms.contains(room))
                            .cloned()
                            .collect();

                        log::info!("Handshake complete. Active rooms: {:?}", intersection);
                        self.state = HandshakeState::Complete {
                            active_rooms: intersection,
                        };
                        Ok(None) // No response frame is needed.
                    }
                    HandshakeState::Complete { .. } => {
                        log::warn!(
                            "Received a Hello frame after handshake was already complete. Ignoring."
                        );
                        Ok(None)
                    }
                    HandshakeState::Failed(..) => {
                        Err(ZzNetConnectionError::InvalidState(
                            "Received a frame while in a Failed state.".to_string(),
                        ))
                    }
                }
            }
            Frame::Room(_) => Err(ZzNetConnectionError::InvalidState(
                "Received Room frame during handshake".to_string(),
            )),
        }
    }
}

impl Default for Handshake {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() {
        let _ = env_logger::builder()
            .is_test(true)
            .filter_level(log::LevelFilter::Debug)
            .try_init();
    }

    #[test]
    fn test_serialization_handshake_frame() {
        setup();
        log::info!("Starting test_serialization_handshake_frame");
        let frame = Frame::Handshake(HandshakeFrame::Hello {
            protocol_version: "1.0".to_string(),
            auth_role: AuthRole::Collector,
            offered_rooms: vec!["room1".to_string(), "room2".to_string()],
        });
        let serialized = serialize(&frame).unwrap();
        let deserialized: Frame = deserialize(&serialized).unwrap();
        assert_eq!(frame, deserialized);
        log::info!("Completed test_serialization_handshake_frame");
    }

    #[test]
    fn test_serialization_room_frame() {
        setup();
        log::info!("Starting test_serialization_room_frame");
        let frame = Frame::Room(RoomFrame::PublishRooms {
            offered_rooms: vec!["room1".to_string()],
        });
        let serialized = serialize(&frame).unwrap();
        let deserialized: Frame = deserialize(&serialized).unwrap();
        assert_eq!(frame, deserialized);
        log::info!("Completed test_serialization_room_frame");
    }

    #[test]
    fn test_handshake_new_starts_in_start_state() {
        setup();
        log::info!("Starting test_handshake_new_starts_in_start_state");
        let handshake = Handshake::new();
        assert_eq!(handshake.state, HandshakeState::Start);
        log::info!("Completed test_handshake_new_starts_in_start_state");
    }

    #[test]
    fn test_create_hello_frame() {
        setup();
        log::info!("Starting test_create_hello_frame");
        let mut handshake = Handshake::new();
        let frame_data = handshake
            .create_hello_frame(
                "1.0".to_string(),
                AuthRole::Collector,
                vec!["room1".to_string(), "room2".to_string()],
            )
            .unwrap();
        let frame: Frame = deserialize(&frame_data).unwrap();
        match frame {
            Frame::Handshake(HandshakeFrame::Hello {
                protocol_version,
                auth_role,
                offered_rooms,
            }) => {
                assert_eq!(protocol_version, "1.0");
                assert_eq!(auth_role, AuthRole::Collector);
                assert_eq!(
                    offered_rooms,
                    vec!["room1".to_string(), "room2".to_string()]
                );
            }
            _ => panic!("Expected Handshake Hello frame"),
        }
        assert!(matches!(handshake.state, HandshakeState::SentHello { .. }));
        log::info!("Completed test_create_hello_frame");
    }

    #[test]
    fn test_process_frame_valid_hello() {
        setup();
        log::info!("Starting test_process_frame_valid_hello");
        let mut handshake = Handshake::new();
        handshake
            .create_hello_frame(
                "1.0".to_string(),
                AuthRole::Collector,
                vec!["intent-config".to_string(), "room2".to_string()],
            )
            .unwrap();

        let peer_frame = Frame::Handshake(HandshakeFrame::Hello {
            protocol_version: "1.0".to_string(),
            auth_role: AuthRole::Database,
            offered_rooms: vec!["intent-config".to_string(), "room3".to_string()],
        });
        let peer_data = serialize(&peer_frame).unwrap();

        let result = handshake.process_frame(peer_data).unwrap();
        assert!(result.is_none());
        assert!(handshake.is_complete());
        assert_eq!(handshake.active_rooms(), Some(&["intent-config".to_string()][..]));
        log::info!("Completed test_process_frame_valid_hello");
    }

    #[test]
    fn test_process_frame_invalid_room() {
        setup();
        log::info!("Starting test_process_frame_invalid_room");
        let mut handshake = Handshake::new();
        handshake
            .create_hello_frame(
                "1.0".to_string(),
                AuthRole::Collector,
                vec!["room1".to_string()],
            )
            .unwrap();

        let room_frame = Frame::Room(RoomFrame::PublishRooms {
            offered_rooms: vec!["room1".to_string()],
        });
        let room_data = serialize(&room_frame).unwrap();

        let result = handshake.process_frame(room_data);
        assert!(result.is_err());
        log::info!("Completed test_process_frame_invalid_room");
    }
}
