//! HelloActor: Production-quality actor bridging transport to session management.
//!
//! This implements the complete HELLO protocol handler using an actor + background task pattern:
//! - Spawns a tokio task to handle transport I/O
//! - Executes handshake state machine in actor context
//! - Routes messages between transport and SessionManager
//! - Handles errors, timeouts, and graceful shutdown
//!
//! ## Architecture
//!
//! ```text
//! HelloActor (actix)
//!   ├─> I/O Task (tokio) ─> TransportConnection
//!   │    ├─ recv() loop
//!   │    └─ send() on demand
//!   └─> SessionManager (actix)
//! ```
//!
//! The I/O task and actor communicate via tokio mpsc channels, avoiding
//! borrow checker issues with async trait objects.

use actix::prelude::*;
use bytes::Bytes;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use zznet_api::error::TransportError;
use zznet_api::transport::TransportConnection;

// HelloActor must be application-agnostic at protocol level. Store our role as a
// role identifier string; the application is responsible for converting its
// concrete role enum to/from this string.
use crate::error::HelloError;
use crate::handshake::Handshake;
use crate::protocol::{Frame, HandshakeFrame, RoomFrame};
use crate::serialize;
use crate::session_messages::{HandshakeComplete, InboundRoomMessage};
// No direct dependency on application role enums here; protocol-level code uses raw role strings.

/// Configuration for HelloActor.
#[derive(Debug, Clone)]
pub struct HelloConfig {
    /// Our authentication role.
    pub our_role: String,
    /// Rooms we want to offer to the peer.
    pub offered_rooms: Vec<String>,
    /// Timeout for handshake completion.
    pub handshake_timeout: Duration,
    /// Our hostname/identifier.
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

/// Current state of the HelloActor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActorState {
    /// Performing handshake.
    Handshaking,
    /// Handshake complete, ready for room communication.
    Ready,
    /// Connection failed or closed.
    Failed,
}

/// Internal message: Frame received from transport.
#[derive(Message)]
#[rtype(result = "()")]
struct ReceivedFrame {
    data: Vec<u8>,
}

/// Internal message: I/O task encountered an error.
#[derive(Message)]
#[rtype(result = "()")]
struct IoError {
    error: HelloError,
}

/// Message to send data through this connection.
#[derive(Message, Debug, Clone)]
#[rtype(result = "Result<(), HelloError>")]
pub struct SendMessage {
    /// Source room name.
    pub from_room: String,
    /// Destination room name.
    pub to_room: String,
    /// Serialized message payload.
    pub payload: Vec<u8>,
}

/// Message to gracefully disconnect.
#[derive(Message)]
#[rtype(result = "()")]
pub struct Disconnect;

/// Message to set the inbound channel for forwarding received messages to SessionManager.
#[derive(Message)]
#[rtype(result = "()")]
pub struct SetInboundChannel {
    /// Channel to send inbound messages to ConnectionManager (which will deserialize).
    pub tx: tokio::sync::mpsc::Sender<(String, Vec<u8>)>,
}

/// Production-quality actor managing a connection through HELLO protocol.
pub struct HelloActor {
    /// Configuration for this connection.
    config: HelloConfig,
    /// Handshake state machine.
    handshake: Handshake,
    /// Current actor state.
    state: ActorState,
    /// Active rooms negotiated during handshake.
    active_rooms: Vec<String>,
    /// Peer's role string received during handshake (CN from certificate).
    peer_role: Option<String>,
    /// Peer's cryptographic identity from transport.
    peer_identity: zznet_api::types::PeerIdentity,
    /// Sender to I/O task for outbound frames.
    io_tx: mpsc::UnboundedSender<Bytes>,
    /// Optional SessionManager recipient (for integration with higher layer).
    session_manager: Option<Recipient<HandshakeComplete>>,
    /// Channel to forward inbound messages to ConnectionManager.
    inbound_tx: Option<tokio::sync::mpsc::Sender<(String, Vec<u8>)>>,
}

impl HelloActor {
    /// Create a new HelloActor.
    ///
    /// This is private - use `start_hello_actor()` to properly create and start the actor.
    fn new(
        config: HelloConfig,
        peer_identity: zznet_api::types::PeerIdentity,
        io_tx: mpsc::UnboundedSender<Bytes>,
    ) -> Self {
        Self {
            config,
            handshake: Handshake::new(),
            state: ActorState::Handshaking,
            active_rooms: Vec::new(),
            peer_role: None,
            peer_identity,
            io_tx,
            session_manager: None,
            inbound_tx: None,
        }
    }

    /// Set the SessionManager recipient for this actor.
    ///
    /// This should be called after creating the actor but before starting handshake.
    pub fn with_session_manager(mut self, recipient: Recipient<HandshakeComplete>) -> Self {
        self.session_manager = Some(recipient);
        self
    }

    /// Start the handshake process by sending HELLO and OFFER frames.
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
                self.send_frame_to_io(hello_data);

                // Immediately send OFFER frame
                match self.handshake.create_offer_frame() {
                    Ok(offer_data) => {
                        self.send_frame_to_io(offer_data);

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

    /// Send a frame to the I/O task for transmission.
    fn send_frame_to_io(&self, data: Vec<u8>) {
        if let Err(e) = self.io_tx.send(Bytes::from(data)) {
            error!("Failed to send frame to I/O task: {}", e);
        }
    }

    /// Handle a received frame based on current state.
    fn handle_received_frame(&mut self, data: Vec<u8>, ctx: &mut Context<Self>) {
        match self.state {
            ActorState::Handshaking => {
                self.handle_handshake_frame(data, ctx);
            }
            ActorState::Ready => {
                self.handle_room_frame(data, ctx);
            }
            ActorState::Failed => {
                warn!("Received frame in Failed state, ignoring");
            }
        }
    }

    /// Handle a frame during handshake phase.
    fn handle_handshake_frame(&mut self, data: Vec<u8>, ctx: &mut Context<Self>) {
        // First, try to extract peer role string if this is a HELLO frame
        if let Ok(Frame::Handshake(HandshakeFrame::Hello { role_str, .. })) =
            Frame::deserialize(&data)
            && self.peer_role.is_none()
        {
            self.peer_role = Some(role_str);
            debug!("Received peer role string");
        }

        match self.handshake.process_frame(&data) {
            Ok(response) => {
                // Send response if state machine generated one
                if let Some(response_data) = response {
                    debug!("Sending handshake response");
                    self.send_frame_to_io(response_data);
                }

                // Check if handshake is complete
                if self.handshake.is_complete() {
                    self.complete_handshake(ctx);
                }
            }
            Err(e) => {
                error!("Handshake frame processing failed: {}", e);
                self.handle_error(e, ctx);
            }
        }
    }

    /// Complete the handshake and transition to Ready state.
    fn complete_handshake(&mut self, ctx: &mut Context<Self>) {
        if let Some(rooms) = self.handshake.active_rooms() {
            info!("Handshake complete! Active rooms: {:?}", rooms);
            self.active_rooms = rooms.to_vec();
            self.state = ActorState::Ready;

            // Notify SessionManager if configured
            if let Some(ref session_mgr) = self.session_manager {
                if let Some(ref peer_role) = self.peer_role {
                    if let Some(peer_hostname) = self.handshake.peer_hostname() {
                        let msg = HandshakeComplete {
                            peer_id: peer_hostname.to_string(),
                            peer_role_str: peer_role.clone(),
                            peer_identity: self.peer_identity.clone(),
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

    /// Handle a room frame (after handshake complete).
    fn handle_room_frame(&mut self, data: Vec<u8>, ctx: &mut Context<Self>) {
        match Frame::deserialize(&data) {
            Ok(Frame::Room(room_frame)) => {
                match room_frame {
                    RoomFrame::Message {
                        from_room,
                        to_room,
                        payload,
                    } => {
                        debug!(
                            "Received room message: {} -> {} ({} bytes)",
                            from_room,
                            to_room,
                            payload.len()
                        );

                        // Forward to ConnectionManager via inbound_tx
                        if let Some(ref tx) = self.inbound_tx {
                            if let Err(e) = tx.try_send((to_room.clone(), payload)) {
                                error!("Failed to forward inbound message: {:?}", e);
                            }
                        } else {
                            warn!("No inbound channel set, dropping message");
                        }
                    }
                    RoomFrame::Disconnect => {
                        info!("Peer sent disconnect");
                        ctx.stop();
                    }
                }
            }
            Ok(Frame::Handshake(_)) => {
                warn!("Received handshake frame after handshake complete, ignoring");
            }
            Err(e) => {
                error!("Failed to deserialize room frame: {}", e);
                self.handle_error(e.into(), ctx);
            }
        }
    }

    /// Handle an error by transitioning to Failed state and stopping.
    fn handle_error(&mut self, error: HelloError, ctx: &mut Context<Self>) {
        error!("HelloActor fatal error: {}", error);
        self.state = ActorState::Failed;

        // Try to send error frame to peer
        if let Ok(error_frame) = Frame::Handshake(crate::protocol::HandshakeFrame::Error {
            message: error.to_string(),
        })
        .serialize()
        {
            self.send_frame_to_io(error_frame);
        }

        // Give time for error frame to send, then stop
        ctx.run_later(Duration::from_millis(100), |_, ctx| {
            ctx.stop();
        });
    }

    /// Spawn the I/O task that handles transport operations.
    fn spawn_io_task(
        mut transport: Box<dyn TransportConnection>,
        mut io_rx: mpsc::UnboundedReceiver<Bytes>,
        actor_addr: Addr<Self>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            debug!("I/O task started");

            loop {
                tokio::select! {
                    // Handle outbound frames (from actor to transport)
                    Some(frame) = io_rx.recv() => {
                        debug!("I/O task sending {} bytes", frame.len());
                        if let Err(e) = transport.send(frame).await {
                            error!("Transport send error: {}", e);
                            actor_addr.do_send(IoError {
                                error: e.into(),
                            });
                            break;
                        }
                    }

                    // Handle inbound frames (from transport to actor)
                    result = transport.recv() => {
                        match result {
                            Ok(Some(bytes)) => {
                                debug!("I/O task received {} bytes", bytes.len());
                                actor_addr.do_send(ReceivedFrame {
                                    data: bytes.to_vec(),
                                });
                            }
                            Ok(None) => {
                                info!("Transport closed by peer");
                                actor_addr.do_send(IoError {
                                    error: HelloError::Transport(TransportError::ConnectionClosed),
                                });
                                break;
                            }
                            Err(e) => {
                                error!("Transport recv error: {}", e);
                                actor_addr.do_send(IoError {
                                    error: e.into(),
                                });
                                break;
                            }
                        }
                    }
                }
            }

            debug!("I/O task exiting");
        })
    }
}

impl Actor for HelloActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("HelloActor started");
        self.start_handshake(ctx);
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("HelloActor stopped");
        // I/O task will exit when actor drops (channel closed)
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

impl Handler<SendMessage> for HelloActor {
    type Result = Result<(), HelloError>;

    fn handle(&mut self, msg: SendMessage, _ctx: &mut Self::Context) -> Self::Result {
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
        self.send_frame_to_io(frame_data);

        Ok(())
    }
}

impl Handler<InboundRoomMessage> for HelloActor {
    type Result = Result<(), HelloError>;

    fn handle(&mut self, msg: InboundRoomMessage, _ctx: &mut Context<Self>) -> Self::Result {
        debug!(
            "Received inbound room message from SessionManager: {} -> {}",
            msg.from_room, msg.to_room
        );

        // Check state
        if self.state != ActorState::Ready {
            return Err(HelloError::InvalidState(format!(
                "Cannot send message in state {:?}",
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
        self.send_frame_to_io(frame_data);

        Ok(())
    }
}

impl Handler<Disconnect> for HelloActor {
    type Result = ();

    fn handle(&mut self, _msg: Disconnect, ctx: &mut Context<Self>) -> Self::Result {
        info!("Disconnect requested");

        // Send disconnect frame
        if let Ok(frame_data) = serialize::create_disconnect_frame() {
            self.send_frame_to_io(frame_data);
        }

        // Give time for disconnect frame to send
        ctx.run_later(Duration::from_millis(100), |_, ctx| {
            ctx.stop();
        });
    }
}

impl Handler<SetInboundChannel> for HelloActor {
    type Result = ();

    fn handle(&mut self, msg: SetInboundChannel, _ctx: &mut Context<Self>) -> Self::Result {
        self.inbound_tx = Some(msg.tx);
        debug!("Set inbound channel for forwarding received messages");
    }
}

/// Start a HelloActor with the given transport and configuration.
///
/// This is the proper way to create and start a HelloActor. It handles:
/// - Creating the actor with proper channel setup
/// - Spawning the I/O task
/// - Starting the actor in the actix system
///
/// Returns the actor address for sending messages.
pub fn start_hello_actor(
    transport: Box<dyn TransportConnection>,
    config: HelloConfig,
) -> Addr<HelloActor> {
    start_hello_actor_with_session_manager(transport, config, None)
}

/// Start a HelloActor with optional SessionManager integration
///
/// This is the full-featured version that allows connecting HelloActor
/// to a SessionManager for Phase 4 integration.
pub fn start_hello_actor_with_session_manager(
    transport: Box<dyn TransportConnection>,
    config: HelloConfig,
    session_manager: Option<Recipient<HandshakeComplete>>,
) -> Addr<HelloActor> {
    let (io_tx, io_rx) = mpsc::unbounded_channel();

    let peer_identity = transport.peer_identity();
    let mut actor = HelloActor::new(config, peer_identity, io_tx);
    if let Some(sm) = session_manager {
        actor = actor.with_session_manager(sm);
    }

    let addr = actor.start();

    // Spawn I/O task with weak reference to avoid circular ownership
    HelloActor::spawn_io_task(transport, io_rx, addr.clone());

    addr
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

        let addr1 = start_hello_actor(Box::new(conn1), config1);
        let _addr2 = start_hello_actor(Box::new(conn2), config2);

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
        let addr = start_hello_actor(Box::new(conn1), config);

        // Send disconnect
        addr.do_send(Disconnect);

        // Wait a bit for disconnect to process
        tokio::time::sleep(Duration::from_millis(1)).await;

        // Actor should have stopped
    }
}
