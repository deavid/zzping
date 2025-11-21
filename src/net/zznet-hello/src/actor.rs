//! Implements the HELLO protocol, bridging a transport connection to the session layer.
//!
//! The `HelloActor` spawns a dedicated Tokio task for transport I/O,
//! communicating with it via MPSC channels. This isolates blocking I/O from
//! the actor's single-threaded context.

use actix::prelude::*;
use bytes::Bytes;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use tokio_stream::wrappers::ReceiverStream;
use zznet_api::error::TransportError;
use zznet_api::transport::TransportConnection;

// The HELLO protocol is application-agnostic; it uses a role string, not a concrete enum.
use crate::error::HelloError;
use crate::handshake::Handshake;
use crate::session_messages::HandshakeComplete;
use zznet_api::protocol::{Frame, HandshakeFrame, RoomFrame};

/// Configures a `HelloActor`.
#[derive(Debug, Clone)]
pub struct HelloConfig {
    /// This service's role.
    pub our_role: String,
    /// Rooms this service offers to peers.
    pub offered_rooms: Vec<String>,
    /// Handshake timeout.
    pub handshake_timeout: Duration,
    /// This service's hostname.
    pub hostname: String,
}

impl Default for HelloConfig {
    fn default() -> Self {
        Self {
            our_role: "collector".to_string(),
            offered_rooms: vec!["intent-config".to_string()],
            handshake_timeout: Duration::from_secs(10),
            hostname: "default-hostname".to_string(),
        }
    }
}

/// `HelloActor` state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ActorState {
    /// Handshaking with peer.
    Handshaking,
    /// Ready for room communication.
    Ready,
    /// Terminal state after failure or closure.
    Failed,
}

/// A frame received from the transport.
#[derive(Message)]
#[rtype(result = "()")]
struct ReceivedFrame {
    data: Vec<u8>,
}

/// An error from the I/O task.
#[derive(Message)]
#[rtype(result = "()")]
struct IoError {
    error: HelloError,
}

/// Sends a message to a room via this connection.
#[derive(Message, Debug, Clone)]
#[rtype(result = "Result<(), HelloError>")]
pub(crate) struct SendMessage {
    /// The source room.
    pub from_room: String,
    /// The destination room.
    pub to_room: String,
    /// The message payload.
    pub payload: Vec<u8>,
}

/// Requests a graceful disconnect.
#[derive(Message)]
#[rtype(result = "()")]
pub(crate) struct Disconnect;

/// Transport handles for direct Router connection
pub(crate) struct TransportHandles {
    pub transport_tx: mpsc::Sender<Bytes>,
    pub transport_rx: mpsc::Receiver<Result<Bytes, TransportError>>,
}

/// Requests transport handles after handshake complete.
/// This transfers ownership of the transport channels to the caller.
#[derive(Message)]
#[rtype(result = "Result<TransportHandles, String>")]
pub(crate) struct GetTransportHandles;

/// Manages a connection's lifecycle using the HELLO protocol.
pub(crate) struct HelloActor {
    /// Connection configuration.
    config: HelloConfig,
    /// The HELLO protocol state machine.
    handshake: Handshake,
    /// The actor's current state.
    state: ActorState,
    /// Rooms negotiated during handshake.
    active_rooms: Vec<String>,
    // FIXME: peer_role MUST NOT be an Option<T>, it is mandatory.
    /// Peer's role string received during handshake.
    peer_role: Option<String>,
    /// TLS peer identity from transport (None for plain TCP).
    /// Used to validate that HELLO role matches certificate CN when TLS is enabled.
    tls_peer_identity: Option<zznet_api::types::PeerTLSIdentity>,
    /// Sender to transport for outbound frames.
    transport_tx: mpsc::Sender<Bytes>,
    /// Optional SessionManager recipient (for integration with higher layer). - FIXME: Why is this optional? it doesn't make sense
    session_manager: Option<Recipient<HandshakeComplete>>,
    /// Receiver for inbound frames from transport.
    transport_rx: Option<mpsc::Receiver<Result<Bytes, TransportError>>>,
}

impl HelloActor {
    /// This is private - use `start_hello_actor()` to properly create and start the actor.
    fn new(
        config: HelloConfig,
        tls_peer_identity: Option<zznet_api::types::PeerTLSIdentity>,
        transport_tx: mpsc::Sender<Bytes>,
        transport_rx: mpsc::Receiver<Result<Bytes, TransportError>>,
    ) -> Self {
        Self {
            config,
            handshake: Handshake::new(),
            state: ActorState::Handshaking,
            active_rooms: Vec::new(),
            peer_role: None,
            tls_peer_identity,
            transport_tx,
            session_manager: None,
            transport_rx: Some(transport_rx),
        }
    }

    /// Sets the recipient for the `HandshakeComplete` message.
    pub(crate) fn with_session_manager(mut self, recipient: Recipient<HandshakeComplete>) -> Self {
        self.session_manager = Some(recipient);
        self
    }

    /// Sends HELLO and OFFER frames to initiate the handshake.
    fn start_handshake(&mut self, ctx: &mut Context<Self>) {
        debug!(
            "Starting HELLO handshake, role={}, rooms={:?}",
            self.config.our_role, self.config.offered_rooms
        );

        // Create and send HELLO frame
        match self.handshake.create_hello_frame(
            self.config.our_role.clone(),
            self.config.offered_rooms.clone(),
            self.config.hostname.clone(),
        ) {
            Ok(hello_data) => {
                self.send_frame_to_transport(hello_data, ctx);

                // Immediately send OFFER frame
                match self.handshake.create_offer_frame() {
                    Ok(offer_data) => {
                        self.send_frame_to_transport(offer_data, ctx);

                        // Schedule handshake timeout
                        ctx.run_later(self.config.handshake_timeout, |act, ctx| {
                            if act.state == ActorState::Handshaking {
                                warn!("Handshake timeout after {:?}", act.config.handshake_timeout);
                                act.handle_error(
                                    HelloError::HandshakeFailed("Timeout".to_string()),
                                    ctx,
                                );
                            }
                        });
                    }
                    Err(e) => {
                        error!("Failed to create OFFER frame: {}", e);
                        self.handle_error(e, ctx);
                    }
                }
            }
            Err(e) => {
                error!("Failed to create HELLO frame: {}", e);
                self.handle_error(e, ctx);
            }
        }
    }

    /// Sends a raw frame to the transport.
    fn send_frame_to_transport(&mut self, data: Vec<u8>, ctx: &mut Context<Self>) {
        if let Err(e) = self.transport_tx.try_send(Bytes::from(data)) {
            error!("Transport channel closed: {}", e);
            ctx.stop();
        }
    }

    /// Routes a received frame based on the current actor state.
    fn handle_received_frame(&mut self, data: Vec<u8>, ctx: &mut Context<Self>) {
        match self.state {
            ActorState::Handshaking => {
                self.handle_handshake_frame(data, ctx);
            }
            ActorState::Ready => {
                // After handshake, transport is transferred to Router.
                // Any frames received here are protocol violations or race conditions.
                warn!("Received frame in Ready state - transport should have been transferred");
            }
            ActorState::Failed => {
                warn!("Received frame in Failed state, ignoring");
            }
        }
    }

    /// Processes a frame during the `Handshaking` state.
    fn handle_handshake_frame(&mut self, data: Vec<u8>, ctx: &mut Context<Self>) {
        // First, try to extract peer role string if this is a HELLO frame
        if let Ok(Frame::Handshake(HandshakeFrame::Hello { role_str, .. })) =
            Frame::deserialize(&data)
            && self.peer_role.is_none()
        {
            debug!("Received peer role string: {:?}", role_str);
            self.peer_role = Some(role_str);
        }

        match self.handshake.process_frame(&data) {
            Ok(response) => {
                // Send response if state machine generated one
                if let Some(response_data) = response {
                    debug!("Sending handshake response");
                    self.send_frame_to_transport(response_data, ctx);
                }

                // Check if handshake is complete
                if self.handshake.is_complete() {
                    self.complete_handshake(ctx);
                }
            }
            Err(e) => {
                error!("Handshake frame processing failed: {}", e);
                // TODO: In `handle_handshake_frame`, if `self.handshake.process_frame()` returns `Err`, call `self.handle_error()` to terminate the actor.
                self.handle_error(e, ctx);
            }
        }
    }

    /// Transitions to `Ready` state and notifies the `SessionManager`.
    // TODO: Add a test case that uses a TLS-enabled transport to cover certificate validation logic in `complete_handshake`.
    fn complete_handshake(&mut self, ctx: &mut Context<Self>) {
        if let Some(rooms) = self.handshake.active_rooms() {
            info!("Handshake complete! Active rooms: {:?}", rooms);
            self.active_rooms = rooms.to_vec();
            self.state = ActorState::Ready;

            // Notify SessionManager if configured
            if let Some(ref session_mgr) = self.session_manager {
                if let Some(ref peer_role) = self.peer_role {
                    // TLS VALIDATION: If TLS is enabled, the certificate role (OU) must match the HELLO role
                    if let Some(ref tls_identity) = self.tls_peer_identity {
                        if tls_identity.role != *peer_role {
                            error!(
                                "TLS validation FAILED: Certificate role '{}' does not match HELLO role '{}'",
                                tls_identity.role, peer_role
                            );
                            self.handle_error(
                                HelloError::HandshakeFailed(format!(
                                    "TLS certificate role '{}' does not match HELLO role '{}'",
                                    tls_identity.role, peer_role
                                )),
                                ctx,
                            );
                            return;
                        }
                        info!(
                            "TLS validation SUCCESS: Certificate role '{}' matches HELLO role",
                            tls_identity.role
                        );
                    } else {
                        debug!(
                            "No TLS identity available - plain TCP connection, no validation performed"
                        );
                    }

                    if let Some(peer_hostname) = self.handshake.peer_hostname() {
                        let msg = HandshakeComplete {
                            peer_id: peer_hostname.to_string(),
                            peer_role_str: peer_role.clone(),
                            active_rooms: self.active_rooms.clone(),
                            hello_actor: ctx.address(),
                        };

                        session_mgr.do_send(msg);
                        debug!(
                            "Notified SessionManager of handshake completion for peer {}",
                            peer_hostname
                        );
                    } else {
                        warn!("Handshake complete but peer_hostname not available");
                    }
                } else {
                    warn!("Handshake complete but peer_role not available");
                }
            } else {
                debug!("No SessionManager configured, running standalone");
            }
        } else {
            error!("Handshake marked complete but no active rooms");
            self.state = ActorState::Failed;
        }
    }

    /// Logs an error, sends an error frame to the peer, and stops the actor.
    // TODO (Architectural Review): Is sending an Error frame necessary? This adds complexity. Consider simplifying to just log and stop.
    fn handle_error(&mut self, error: HelloError, ctx: &mut Context<Self>) {
        error!("HelloActor fatal error: {}", error);
        self.state = ActorState::Failed;

        // Try to send error frame to peer
        if let Ok(error_frame) = Frame::Handshake(zznet_api::protocol::HandshakeFrame::Error {
            message: error.to_string(),
        })
        .serialize()
        {
            self.send_frame_to_transport(error_frame, ctx);
        }

        // Give time for error frame to send, then stop
        ctx.run_later(Duration::from_millis(100), |_, ctx| {
            ctx.stop();
        });
    }
}

impl Actor for HelloActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("HelloActor started");
        // Add the transport stream if available
        if let Some(rx) = self.transport_rx.take() {
            ctx.add_stream(ReceiverStream::new(rx));
        }
        self.start_handshake(ctx);
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("HelloActor stopped");
        // Transport tasks will exit when channels are closed
    }
}

impl Handler<ReceivedFrame> for HelloActor {
    type Result = ();

    fn handle(&mut self, msg: ReceivedFrame, ctx: &mut Context<Self>) -> Self::Result {
        self.handle_received_frame(msg.data, ctx);
    }
}

impl Handler<IoError> for HelloActor {
    type Result = ();

    fn handle(&mut self, msg: IoError, ctx: &mut Context<Self>) -> Self::Result {
        self.handle_error(msg.error, ctx);
    }
}

impl StreamHandler<Result<Bytes, TransportError>> for HelloActor {
    fn handle(&mut self, item: Result<Bytes, TransportError>, ctx: &mut Context<Self>) {
        match item {
            Ok(data) => self.handle_received_frame(data.to_vec(), ctx),
            Err(e) => {
                if matches!(e, TransportError::ConnectionClosed(_)) {
                    info!("Transport closed by peer");
                } else {
                    error!("Transport recv error: {}", e);
                }
                self.handle_error(e.into(), ctx);
            }
        }
    }

    fn finished(&mut self, ctx: &mut Self::Context) {
        info!("Transport stream finished");
        ctx.stop();
    }
}

impl Handler<SendMessage> for HelloActor {
    type Result = Result<(), HelloError>;

    fn handle(&mut self, msg: SendMessage, ctx: &mut Context<Self>) -> Self::Result {
        if self.state != ActorState::Ready {
            return Err(HelloError::InvalidState(format!(
                "Cannot send in state {:?}",
                self.state
            )));
        }

        // Verify room is active
        if !self.active_rooms.contains(&msg.from_room) {
            return Err(HelloError::InvalidState(format!(
                "Room '{}' not in active rooms: {:?}",
                msg.from_room, self.active_rooms
            )));
        }

        // Create and send room frame
        let room_frame = Frame::Room(RoomFrame::Message {
            from_room: msg.from_room.clone(),
            to_room: msg.to_room.clone(),
            payload: msg.payload,
        });

        let frame_data = room_frame.serialize()?;
        debug!("Sending room message: {} -> {}", msg.from_room, msg.to_room);
        self.send_frame_to_transport(frame_data, ctx);

        Ok(())
    }
}

impl Handler<Disconnect> for HelloActor {
    type Result = ();

    fn handle(&mut self, _msg: Disconnect, ctx: &mut Context<Self>) -> Self::Result {
        info!("Disconnect requested");

        // Send disconnect frame
        let frame = Frame::Room(RoomFrame::Disconnect);
        if let Ok(frame_data) = frame.serialize() {
            self.send_frame_to_transport(frame_data, ctx);
        }

        // Give time for disconnect frame to send
        ctx.run_later(Duration::from_millis(100), |_, ctx| {
            ctx.stop();
        });
    }
}

impl Handler<GetTransportHandles> for HelloActor {
    type Result = Result<TransportHandles, String>;

    fn handle(&mut self, _msg: GetTransportHandles, ctx: &mut Context<Self>) -> Self::Result {
        // Can only transfer handles if we're in Ready state and still have them
        if self.state != ActorState::Ready {
            return Err(
                "Transport handles can only be extracted after handshake complete".to_string(),
            );
        }

        let transport_rx = self
            .transport_rx
            .take()
            .ok_or_else(|| "Transport receiver already transferred".to_string())?;

        // Stop the actor since transport is being transferred
        ctx.stop();

        Ok(TransportHandles {
            transport_tx: self.transport_tx.clone(),
            transport_rx,
        })
    }
}

/// Creates and starts a `HelloActor` and its associated I/O task.
///
/// This is the primary entry point for creating a `HelloActor`. It wires up the
/// actor, its I/O task, and the transport, returning the actor's address.
pub(crate) fn start_hello_actor_with_session_manager(
    transport: Box<dyn TransportConnection>,
    config: HelloConfig,
    session_manager: Option<Recipient<HandshakeComplete>>,
) -> Addr<HelloActor> {
    let peer_addr = transport.peer_addr();
    // Extract TLS peer identity from transport (if available)
    let tls_peer_identity = transport.peer_tls_identity();
    if let Some(ref identity) = tls_peer_identity {
        debug!(
            "TLS connection detected: role='{}', username='{}', addr='{:?}'",
            identity.role, identity.username, peer_addr
        );
    } else {
        debug!("Connection with no TLS identity, addr='{peer_addr:?}'");
    }

    // Start the transport and get channels
    let (transport_tx, transport_rx) = transport.start();

    let mut actor = HelloActor::new(config, tls_peer_identity, transport_tx, transport_rx);
    if let Some(sm) = session_manager {
        actor = actor.with_session_manager(sm);
    }

    actor.start()
}

#[cfg(test)]
mod tests {
    use super::*;
    use zznet_api::mock::create_mock_pair;

    #[test]
    fn test_hello_config_default() {
        let config = HelloConfig::default();
        assert_eq!(config.our_role, "collector".to_string());
        assert_eq!(config.offered_rooms, vec!["intent-config"]);
        assert_eq!(config.handshake_timeout, Duration::from_secs(10));
    }

    #[test]
    fn test_hello_config_custom() {
        let config = HelloConfig {
            our_role: "database".to_string(),
            offered_rooms: vec!["memdb".to_string(), "query".to_string()],
            handshake_timeout: Duration::from_secs(5),
            hostname: "custom-host".to_string(),
        };

        assert_eq!(config.our_role, "database".to_string());
        assert_eq!(config.offered_rooms.len(), 2);
        assert_eq!(config.handshake_timeout, Duration::from_secs(5));
        assert_eq!(config.hostname, "custom-host");
    }

    #[test]
    fn test_actor_states() {
        assert_eq!(ActorState::Handshaking, ActorState::Handshaking);
        assert_ne!(ActorState::Handshaking, ActorState::Ready);
        assert_ne!(ActorState::Ready, ActorState::Failed);
    }

    #[test]
    fn test_send_message_structure() {
        let msg = SendMessage {
            from_room: "memdb".to_string(),
            to_room: "query".to_string(),
            payload: vec![1, 2, 3, 4],
        };

        assert_eq!(msg.from_room, "memdb");
        assert_eq!(msg.to_room, "query");
        assert_eq!(msg.payload.len(), 4);
    }

    #[actix::test]
    async fn test_full_handshake_with_mock_transport() {
        use tokio::time::Duration;

        let (conn1, conn2) = create_mock_pair("test1");

        let config1 = HelloConfig {
            our_role: "collector".to_string(),
            offered_rooms: vec!["memdb".to_string(), "query".to_string()],
            handshake_timeout: Duration::from_millis(10),
            hostname: "host1".to_string(),
        };

        let config2 = HelloConfig {
            our_role: "database".to_string(),
            offered_rooms: vec!["memdb".to_string(), "stats".to_string()],
            handshake_timeout: Duration::from_millis(10),
            hostname: "host2".to_string(),
        };

        let addr1 = start_hello_actor_with_session_manager(Box::new(conn1), config1, None);
        let _addr2 = start_hello_actor_with_session_manager(Box::new(conn2), config2, None);

        // Wait for handshake to complete (both should finish)
        // In reality, they should complete quickly
        tokio::time::sleep(Duration::from_millis(1)).await;

        // Try sending a message (this tests the full flow)
        let result = addr1
            .send(SendMessage {
                from_room: "memdb".to_string(),
                to_room: "memdb".to_string(),
                payload: vec![1, 2, 3],
            })
            .await;

        // Should succeed after handshake
        assert!(result.is_ok());
    }

    #[actix::test]
    async fn test_disconnect() {
        let (conn1, _conn2) = create_mock_pair("test2");

        let config = HelloConfig::default();
        let addr = start_hello_actor_with_session_manager(Box::new(conn1), config, None);

        // Send disconnect
        addr.do_send(Disconnect);

        // Wait a bit for disconnect to process
        tokio::time::sleep(Duration::from_millis(1)).await;

        // Actor should have stopped
    }
}
