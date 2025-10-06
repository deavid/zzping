//! Pure state machine logic for the HELLO protocol handshake.
//!
//! This module implements a symmetric handshake where either peer can initiate.
//! The state machine is deliberately isolated from I/O and actor concerns,
//! making it easy to unit test.
//!
//! Handshake sequence:
//! 1. Initiator sends HELLO (version, role, offered rooms)
//! 2. Responder sends HELLO (version, role, offered rooms)
//! 3. Both sides compute intersection of offered rooms
//! 4. Both sides filter by authorization (role can access room)
//! 5. Handshake complete with active rooms
//!
//! Note: The current implementation is simplified compared to the full OFFER/ACK
//! sequence described in the protocol module. This matches the existing working
//! implementation from zznet-connection.

use crate::auth::AuthRole;
use crate::error::HelloError;
use crate::protocol::{Frame, HandshakeFrame};

/// Current state of the handshake process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandshakeState {
    /// Initial state - no messages sent or received yet.
    Start,

    /// We sent HELLO, waiting for peer's HELLO.
    SentHello {
        /// Rooms we offered to the peer.
        our_offered_rooms: Vec<String>,
    },

    /// Handshake completed successfully.
    Complete {
        /// Negotiated rooms both peers will use.
        active_rooms: Vec<String>,
    },

    /// Handshake failed.
    Failed(String),
}

/// State machine for the HELLO handshake protocol.
///
/// This is a pure state machine with no I/O. Call methods to transition
/// between states and generate frames to send.
#[derive(Debug, Clone)]
pub struct Handshake {
    state: HandshakeState,
    protocol_version: String,
    peer_hostname: Option<String>,
}

impl Handshake {
    /// Protocol version constant.
    pub const PROTOCOL_VERSION: &'static str = "1.0";

    /// Create a new handshake in the Start state.
    pub fn new() -> Self {
        Self {
            state: HandshakeState::Start,
            protocol_version: Self::PROTOCOL_VERSION.to_string(),
            peer_hostname: None,
        }
    }

    /// Check if handshake has completed successfully.
    pub fn is_complete(&self) -> bool {
        matches!(self.state, HandshakeState::Complete { .. })
    }

    /// Check if handshake has failed.
    pub fn is_failed(&self) -> bool {
        matches!(self.state, HandshakeState::Failed(_))
    }

    /// Get the list of active rooms if handshake is complete.
    pub fn active_rooms(&self) -> Option<&[String]> {
        match &self.state {
            HandshakeState::Complete { active_rooms } => Some(active_rooms),
            _ => None,
        }
    }

    /// Get current state (for debugging/logging).
    pub fn state(&self) -> &HandshakeState {
        &self.state
    }

    /// Get the peer hostname if available.
    pub fn peer_hostname(&self) -> Option<&str> {
        self.peer_hostname.as_deref()
    }

    /// Create the initial HELLO frame to send to peer.
    ///
    /// Transitions from Start → SentHello.
    /// Returns the serialized frame ready to send.
    pub fn create_hello_frame(
        &mut self,
        role: AuthRole,
        offered_rooms: Vec<String>,
        hostname: String,
    ) -> Result<Vec<u8>, HelloError> {
        match &self.state {
            HandshakeState::Start => {
                let frame = Frame::Handshake(HandshakeFrame::Hello {
                    version: self.protocol_version.clone(),
                    role,
                    hostname,
                });

                let serialized = frame.serialize()?;

                self.state = HandshakeState::SentHello {
                    our_offered_rooms: offered_rooms,
                };

                Ok(serialized)
            }
            _ => Err(HelloError::InvalidState(format!(
                "Cannot create HELLO from state {:?}",
                self.state
            ))),
        }
    }

    /// Create an OFFER frame to send to peer.
    ///
    /// Must be in SentHello state. Returns serialized frame.
    pub fn create_offer_frame(&self) -> Result<Vec<u8>, HelloError> {
        match &self.state {
            HandshakeState::SentHello { our_offered_rooms } => {
                let frame = Frame::Handshake(HandshakeFrame::Offer {
                    rooms: our_offered_rooms.clone(),
                });
                Ok(frame.serialize()?)
            }
            _ => Err(HelloError::InvalidState(format!(
                "Cannot create OFFER from state {:?}",
                self.state
            ))),
        }
    }

    /// Process an incoming frame from the peer.
    ///
    /// This drives the state machine forward. May return a frame to send in response.
    /// Currently implements simplified handshake (HELLO only, no OFFER/ACK).
    pub fn process_frame(&mut self, frame_data: &[u8]) -> Result<Option<Vec<u8>>, HelloError> {
        let frame = Frame::deserialize(frame_data)?;

        match frame {
            Frame::Handshake(handshake_frame) => self.process_handshake_frame(handshake_frame),
            Frame::Room(_) => Err(HelloError::InvalidState(
                "Received Room frame during handshake".to_string(),
            )),
        }
    }

    fn process_handshake_frame(
        &mut self,
        frame: HandshakeFrame,
    ) -> Result<Option<Vec<u8>>, HelloError> {
        match frame {
            HandshakeFrame::Hello {
                version,
                role,
                hostname,
            } => self.process_hello(version, role, hostname),
            HandshakeFrame::Offer { rooms } => self.process_offer(rooms),
            HandshakeFrame::Ack { rooms } => self.process_ack(rooms),
            HandshakeFrame::Error { message } => {
                self.state = HandshakeState::Failed(message.clone());
                Err(HelloError::HandshakeFailed(message))
            }
        }
    }

    fn process_hello(
        &mut self,
        peer_version: String,
        _peer_role: AuthRole,
        peer_hostname: String,
    ) -> Result<Option<Vec<u8>>, HelloError> {
        // Store the peer hostname
        self.peer_hostname = Some(peer_hostname);
        // Validate protocol version
        if peer_version != self.protocol_version {
            let error_frame = Frame::Handshake(HandshakeFrame::Error {
                message: format!(
                    "Version mismatch: expected {}, got {}",
                    self.protocol_version, peer_version
                ),
            });
            self.state = HandshakeState::Failed("Version mismatch".to_string());
            return Ok(Some(error_frame.serialize()?));
        }

        match &self.state {
            HandshakeState::Start => {
                // Peer sent HELLO before we did - this is an error in our simplified protocol.
                // In a real implementation, we'd reply with our HELLO.
                self.state =
                    HandshakeState::Failed("Received HELLO before sending ours".to_string());
                Err(HelloError::InvalidState(
                    "Received HELLO in Start state - call create_hello_frame first".to_string(),
                ))
            }
            HandshakeState::SentHello { .. } => {
                // This is the expected case - we sent HELLO, now got peer's HELLO.
                // We stay in SentHello state and wait for OFFER frames.
                // The peer will also need to send OFFER with their rooms.

                Ok(None)
            }
            HandshakeState::Complete { .. } => {
                // Already complete, ignore duplicate HELLO
                Ok(None)
            }
            HandshakeState::Failed(_) => Err(HelloError::InvalidState(
                "Cannot process frames in Failed state".to_string(),
            )),
        }
    }

    fn process_offer(&mut self, peer_rooms: Vec<String>) -> Result<Option<Vec<u8>>, HelloError> {
        match &self.state {
            HandshakeState::SentHello { our_offered_rooms } => {
                // Compute intersection of offered rooms
                let intersection: Vec<String> = our_offered_rooms
                    .iter()
                    .filter(|room| peer_rooms.contains(room))
                    .cloned()
                    .collect();

                if intersection.is_empty() {
                    let error_frame = Frame::Handshake(HandshakeFrame::Error {
                        message: "No common rooms".to_string(),
                    });
                    self.state = HandshakeState::Failed("No common rooms".to_string());
                    return Ok(Some(error_frame.serialize()?));
                }

                // Complete handshake with agreed rooms
                self.state = HandshakeState::Complete {
                    active_rooms: intersection.clone(),
                };

                // Send ACK with agreed rooms
                let ack_frame = Frame::Handshake(HandshakeFrame::Ack {
                    rooms: intersection,
                });
                Ok(Some(ack_frame.serialize()?))
            }
            _ => Err(HelloError::InvalidState(format!(
                "Cannot process OFFER in state {:?}",
                self.state
            ))),
        }
    }

    fn process_ack(&mut self, peer_rooms: Vec<String>) -> Result<Option<Vec<u8>>, HelloError> {
        match &self.state {
            HandshakeState::Complete { active_rooms } => {
                // Verify peer agrees on the same rooms
                if active_rooms != &peer_rooms {
                    self.state = HandshakeState::Failed("Room mismatch in ACK".to_string());
                    return Err(HelloError::HandshakeFailed(
                        "Peer ACK'd different rooms than agreed".to_string(),
                    ));
                }
                Ok(None)
            }
            _ => Err(HelloError::InvalidState(format!(
                "Cannot process ACK in state {:?}",
                self.state
            ))),
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

    #[test]
    fn test_new_handshake_starts_in_start_state() {
        let handshake = Handshake::new();
        assert_eq!(handshake.state, HandshakeState::Start);
        assert!(!handshake.is_complete());
        assert!(!handshake.is_failed());
    }

    #[test]
    fn test_create_hello_frame() {
        let mut handshake = Handshake::new();
        let offered_rooms = vec!["memdb".to_string(), "query".to_string()];

        let frame_data = handshake
            .create_hello_frame(
                AuthRole::Collector,
                offered_rooms.clone(),
                "test-host".to_string(),
            )
            .unwrap();

        assert!(matches!(handshake.state, HandshakeState::SentHello { .. }));

        // Verify we can deserialize the frame
        let frame = Frame::deserialize(&frame_data).unwrap();
        match frame {
            Frame::Handshake(HandshakeFrame::Hello {
                version,
                role,
                hostname,
            }) => {
                assert_eq!(version, "1.0");
                assert_eq!(role, AuthRole::Collector);
                assert_eq!(hostname, "test-host");
            }
            _ => panic!("Expected HELLO frame"),
        }
    }

    #[test]
    fn test_create_hello_frame_invalid_state() {
        let mut handshake = Handshake::new();
        handshake
            .create_hello_frame(
                AuthRole::Collector,
                vec!["memdb".to_string()],
                "test-host".to_string(),
            )
            .unwrap();

        // Try to create another HELLO - should fail
        let result = handshake.create_hello_frame(
            AuthRole::Database,
            vec!["memdb".to_string()],
            "test-host2".to_string(),
        );

        assert!(result.is_err());
    }

    #[test]
    fn test_process_hello_version_mismatch() {
        let mut handshake = Handshake::new();
        handshake
            .create_hello_frame(
                AuthRole::Collector,
                vec!["memdb".to_string()],
                "test-host".to_string(),
            )
            .unwrap();

        let bad_frame = Frame::Handshake(HandshakeFrame::Hello {
            version: "2.0".to_string(),
            role: AuthRole::Database,
            hostname: "bad-host".to_string(),
        });
        let bad_data = bad_frame.serialize().unwrap();

        let result = handshake.process_frame(&bad_data).unwrap();
        assert!(result.is_some()); // Should return error frame
        assert!(handshake.is_failed());
    }

    #[test]
    fn test_full_handshake_with_offer_ack() {
        let mut initiator = Handshake::new();
        let mut responder = Handshake::new();

        let init_rooms = vec!["memdb".to_string(), "query".to_string()];
        let resp_rooms = vec!["memdb".to_string(), "stats".to_string()];

        // 1. Initiator sends HELLO
        let hello1 = initiator
            .create_hello_frame(
                AuthRole::Collector,
                init_rooms.clone(),
                "initiator-host".to_string(),
            )
            .unwrap();

        // 2. Responder sends HELLO (independent)
        let hello2 = responder
            .create_hello_frame(
                AuthRole::Database,
                resp_rooms.clone(),
                "responder-host".to_string(),
            )
            .unwrap();

        // 3. Each processes the other's HELLO
        let _ = initiator.process_frame(&hello2).unwrap();
        let _ = responder.process_frame(&hello1).unwrap();

        // 4. Initiator sends OFFER
        let offer1 = initiator.create_offer_frame().unwrap();

        // 5. Responder sends OFFER
        let offer2 = responder.create_offer_frame().unwrap();

        // 6. Initiator processes responder's OFFER and sends ACK
        let ack1_data = initiator.process_frame(&offer2).unwrap().unwrap();
        assert!(initiator.is_complete());
        assert_eq!(initiator.active_rooms(), Some(&["memdb".to_string()][..]));

        // 7. Responder processes initiator's OFFER and sends ACK
        let ack2_data = responder.process_frame(&offer1).unwrap().unwrap();
        assert!(responder.is_complete());
        assert_eq!(responder.active_rooms(), Some(&["memdb".to_string()][..]));

        // 8. Cross-verify ACKs (optional - verify both agree)
        initiator.process_frame(&ack2_data).unwrap();
        responder.process_frame(&ack1_data).unwrap();

        // Both should be complete with same rooms
        assert_eq!(initiator.active_rooms(), responder.active_rooms());
    }

    #[test]
    fn test_no_common_rooms() {
        let mut initiator = Handshake::new();
        let mut responder = Handshake::new();

        let init_rooms = vec!["memdb".to_string()];
        let resp_rooms = vec!["query".to_string()];

        // Exchange HELLOs
        let hello1 = initiator
            .create_hello_frame(AuthRole::Collector, init_rooms, "init-host".to_string())
            .unwrap();
        let hello2 = responder
            .create_hello_frame(AuthRole::Database, resp_rooms, "resp-host".to_string())
            .unwrap();

        let _ = initiator.process_frame(&hello2).unwrap();
        let _ = responder.process_frame(&hello1).unwrap();

        // Send OFFER - should fail with no common rooms
        let offer = initiator.create_offer_frame().unwrap();
        let result = responder.process_frame(&offer).unwrap();

        // Should return error frame
        assert!(result.is_some());
        assert!(responder.is_failed());
    }

    #[test]
    fn test_process_room_frame_during_handshake() {
        let mut handshake = Handshake::new();
        handshake
            .create_hello_frame(
                AuthRole::Collector,
                vec!["memdb".to_string()],
                "test-host".to_string(),
            )
            .unwrap();

        let room_frame = Frame::Room(crate::protocol::RoomFrame::Disconnect);
        let room_data = room_frame.serialize().unwrap();

        let result = handshake.process_frame(&room_data);
        assert!(result.is_err());
    }
}
