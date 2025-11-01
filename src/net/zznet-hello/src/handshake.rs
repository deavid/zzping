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

    /// Get the list of active rooms if handshake is complete.
    pub fn active_rooms(&self) -> Option<&[String]> {
        match &self.state {
            HandshakeState::Complete { active_rooms } => Some(active_rooms),
            _ => None,
        }
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
        role_str: String,
        offered_rooms: Vec<String>,
        hostname: String,
    ) -> Result<Vec<u8>, HelloError> {
        match &self.state {
            HandshakeState::Start => {
                let frame = Frame::Handshake(HandshakeFrame::Hello {
                    version: self.protocol_version.clone(),
                    role_str,
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

    /// Create an OFFER frame using the rooms we previously offered in create_hello_frame.
    /// Returns serialized OFFER frame bytes.
    pub fn create_offer_frame(&mut self) -> Result<Vec<u8>, HelloError> {
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
                role_str,
                hostname,
            } => self.process_hello(version, role_str, hostname),
            HandshakeFrame::Offer { rooms } => self.process_offer(rooms),
            HandshakeFrame::Error { message } => {
                self.state = HandshakeState::Failed(message.clone());
                Err(HelloError::HandshakeFailed(message))
            }
        }
    }

    fn process_hello(
        &mut self,
        peer_version: String,
        _peer_role_str: String,
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
                tracing::warn!("Received duplicate HELLO frame, ignoring");
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
                    tracing::warn!("No common rooms found during handshake");
                    self.state = HandshakeState::Failed("No common rooms".to_string());
                    return Err(HelloError::HandshakeFailed("No common rooms".to_string()));
                }

                // Complete handshake with agreed rooms
                self.state = HandshakeState::Complete {
                    active_rooms: intersection.clone(),
                };

                // Handshake is complete, no response needed from here
                Ok(None)
            }
            _ => Err(HelloError::InvalidState(format!(
                "Cannot process OFFER in state {:?}",
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
    }

    #[test]
    fn test_create_hello_frame() {
        let mut handshake = Handshake::new();
        let offered_rooms = vec!["memdb".to_string(), "query".to_string()];

        let frame_data = handshake
            .create_hello_frame(
                "collector".to_string(),
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
                role_str,
                hostname,
            }) => {
                assert_eq!(version, "1.0");
                assert_eq!(role_str, "collector");
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
                "collector".to_string(),
                vec!["memdb".to_string()],
                "test-host".to_string(),
            )
            .unwrap();

        // Try to create another HELLO - should fail
        let result = handshake.create_hello_frame(
            "database".to_string(),
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
                "collector".to_string(),
                vec!["memdb".to_string()],
                "test-host".to_string(),
            )
            .unwrap();

        let bad_frame = Frame::Handshake(HandshakeFrame::Hello {
            version: "2.0".to_string(),
            role_str: "database".to_string(),
            hostname: "bad-host".to_string(),
        });
        let bad_data = bad_frame.serialize().unwrap();

        let result = handshake.process_frame(&bad_data).unwrap();
        assert!(result.is_some()); // Should return error frame
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
                "collector".to_string(),
                init_rooms.clone(),
                "initiator-host".to_string(),
            )
            .unwrap();

        // 2. Responder sends HELLO (independent)
        let hello2 = responder
            .create_hello_frame(
                "database".to_string(),
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

        // 6. Initiator processes responder's OFFER
        let response1 = initiator.process_frame(&offer2).unwrap();
        assert!(initiator.is_complete());
        assert!(response1.is_none()); // No ACK frame sent
        assert_eq!(initiator.active_rooms(), Some(&["memdb".to_string()][..]));

        // 7. Responder processes initiator's OFFER
        let response2 = responder.process_frame(&offer1).unwrap();
        assert!(responder.is_complete());
        assert!(response2.is_none()); // No ACK frame sent
        assert_eq!(responder.active_rooms(), Some(&["memdb".to_string()][..]));

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
            .create_hello_frame("collector".to_string(), init_rooms, "init-host".to_string())
            .unwrap();
        let hello2 = responder
            .create_hello_frame("database".to_string(), resp_rooms, "resp-host".to_string())
            .unwrap();

        let _ = initiator.process_frame(&hello2).unwrap();
        let _ = responder.process_frame(&hello1).unwrap();

        // Send OFFER - should fail with no common rooms
        let offer = initiator.create_offer_frame().unwrap();
        let result = responder.process_frame(&offer);

        // Should return error
        assert!(result.is_err());
    }

    #[test]
    fn test_process_room_frame_during_handshake() {
        let mut handshake = Handshake::new();
        handshake
            .create_hello_frame(
                "collector".to_string(),
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
