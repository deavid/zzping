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
use zznet_api::{Frame, HandshakeFrame};

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
