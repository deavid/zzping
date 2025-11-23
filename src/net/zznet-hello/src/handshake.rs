//! Implements the HELLO protocol handshake as a pure state machine.
//!
//! This module is isolated from I/O and actor concerns, making it easy to
//! unit test the handshake logic. The handshake is symmetric, allowing either
//! peer to initiate.
//!
//! The sequence is:
//! 1. Both peers send a `HELLO` frame with their version, role, and hostname.
//! 2. Both peers send an `OFFER` frame with the rooms they wish to use.
//! 3. Both sides compute the intersection of offered rooms to determine the
//!    set of `active_rooms` for the session.

use crate::error::HelloError;
use zznet_api::protocol::{Frame, HandshakeFrame};

/// The current state of the handshake process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HandshakeState {
    /// The initial state before any messages are sent or received.
    Start,

    /// State after sending our `HELLO` frame, while waiting for the peer's `HELLO` and `OFFER`.
    SentHello {
        /// The rooms we offered to the peer.
        our_offered_rooms: Vec<String>,
    },

    /// The handshake has completed successfully.
    Complete {
        /// The set of rooms negotiated for the session.
        active_rooms: Vec<String>,
    },

    /// The handshake has failed.
    Failed(String),
}

/// A pure state machine for the HELLO handshake protocol.
///
/// This struct manages state transitions based on method calls and incoming frames,
/// generating outgoing frames as needed. It does not perform any I/O.
#[derive(Debug, Clone)]
pub(crate) struct Handshake {
    state: HandshakeState,
    protocol_version: String,
    peer_hostname: Option<String>,
}

impl Handshake {
    /// The protocol version used for the handshake.
    pub(crate) const PROTOCOL_VERSION: &'static str = "1.0";

    /// Creates a new `Handshake` in the `Start` state.
    pub(crate) fn new() -> Self {
        Self {
            state: HandshakeState::Start,
            protocol_version: Self::PROTOCOL_VERSION.to_string(),
            peer_hostname: None,
        }
    }

    /// True if the handshake finished successfully.
    pub(crate) fn is_complete(&self) -> bool {
        matches!(self.state, HandshakeState::Complete { .. })
    }

    /// Returns the list of active rooms if the handshake is complete.
    pub(crate) fn active_rooms(&self) -> Option<&[String]> {
        match &self.state {
            HandshakeState::Complete { active_rooms } => Some(active_rooms),
            _ => None,
        }
    }

    /// Returns the peer's hostname if the `HELLO` frame has been processed.
    pub(crate) fn peer_hostname(&self) -> Option<&str> {
        self.peer_hostname.as_deref()
    }

    /// Generates the initial HELLO frame and transitions state.
    pub(crate) fn create_hello_frame(
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

                self.state = HandshakeState::SentHello {
                    our_offered_rooms: offered_rooms,
                };

                Ok(frame.serialize()?)
            }
            _ => Err(HelloError::InvalidState(format!(
                "Cannot create HELLO from state {:?}",
                self.state
            ))),
        }
    }

    /// Generates the OFFER frame with local room capabilities.
    pub(crate) fn create_offer_frame(&mut self) -> Result<Vec<u8>, HelloError> {
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

    /// Drives the state machine based on the incoming frame type.
    pub(crate) fn process_frame(
        &mut self,
        frame_data: &[u8],
    ) -> Result<Option<Vec<u8>>, HelloError> {
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
        self.peer_hostname = Some(peer_hostname);

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
                // This indicates the peer sent HELLO before we did. In our simplified
                // symmetric protocol, this is an error because we expect to initiate.
                self.state =
                    HandshakeState::Failed("Received HELLO before sending ours".to_string());
                Err(HelloError::InvalidState(
                    "Received HELLO in Start state - call create_hello_frame first".to_string(),
                ))
            }
            HandshakeState::SentHello { .. } => {
                // We sent HELLO, and now we've received the peer's HELLO.
                // The state remains `SentHello` as we wait for the `OFFER` frame.
                Ok(None)
            }
            HandshakeState::Complete { .. } => {
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

                self.state = HandshakeState::Complete {
                    active_rooms: intersection,
                };

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

        let room_frame = Frame::Room(zznet_api::protocol::RoomFrame::Disconnect);
        let room_data = room_frame.serialize().unwrap();

        let result = handshake.process_frame(&room_data);
        assert!(result.is_err());
    }
}
