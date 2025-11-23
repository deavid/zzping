//! Implements the HELLO protocol, bridging a transport connection to the session layer.
//!
//! The `HelloActor` spawns a dedicated Tokio task for transport I/O,
//! communicating with it via MPSC channels. This isolates blocking I/O from
//! the actor's single-threaded context.

use actix::prelude::*;
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use tokio_stream::wrappers::ReceiverStream;
use zznet_api::{
    Frame, HandshakeFrame, InboundRoomPayload, RoomFrame, RoomId, TransportConnection,
    TransportError, TransportFrame,
};

// The HELLO protocol is application-agnostic; it uses a role string, not a concrete enum.
use crate::error::HelloError;
use crate::handshake::Handshake;
use crate::session_messages::HandshakeComplete;

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
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ActorState {
    /// Handshaking with peer.
    Handshaking,
    /// Ready for room communication (handshake complete, awaiting routes).
    Ready,
    /// Proxying frames to rooms (data plane active).
    Proxy {
        routes: std::collections::HashMap<
            zznet_api::RoomId,
            actix::Recipient<zznet_api::InboundRoomPayload>,
        >,
    },
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

/// Sets the routing table for proxy mode.
/// After receiving this message, HelloActor transitions to Proxy state.
#[derive(Message)]
#[rtype(result = "()")]
pub(crate) struct SetRoutes(
    pub  std::collections::HashMap<
        zznet_api::RoomId,
        actix::Recipient<zznet_api::InboundRoomPayload>,
    >,
);

/// Gets the transport_tx sender for room creation.
#[derive(Message)]
#[rtype(result = "mpsc::Sender<TransportFrame>")]
pub(crate) struct GetTransportTx;

/// Manages a connection's lifecycle using the HELLO protocol.
pub struct HelloActor {
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
    tls_peer_identity: Option<zznet_api::PeerTLSIdentity>,
    /// Sender to transport for outbound frames.
    transport_tx: mpsc::Sender<TransportFrame>,
    /// Optional handshake recipient (for integration with higher layer). - FIXME: Why is this optional? it doesn't make sense
    session_manager: Option<Recipient<HandshakeComplete>>,
    /// Receiver for inbound frames from transport.
    transport_rx: Option<mpsc::Receiver<Result<TransportFrame, TransportError>>>,
}

impl HelloActor {
    /// This is private - use `start_hello_actor()` to properly create and start the actor.
    fn new(
        config: HelloConfig,
        tls_peer_identity: Option<zznet_api::PeerTLSIdentity>,
        transport_tx: mpsc::Sender<TransportFrame>,
        transport_rx: mpsc::Receiver<Result<TransportFrame, TransportError>>,
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

        match self.handshake.create_hello_frame(
            self.config.our_role.clone(),
            self.config.offered_rooms.clone(),
            self.config.hostname.clone(),
        ) {
            Ok(hello_data) => {
                self.send_frame_to_transport(hello_data, ctx);

                match self.handshake.create_offer_frame() {
                    Ok(offer_data) => {
                        self.send_frame_to_transport(offer_data, ctx);

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
        let frame = TransportFrame::new(data);
        if let Err(e) = self.transport_tx.try_send(frame) {
            error!("Transport channel closed: {}", e);
            ctx.stop();
        }
    }

    /// Routes a received frame based on the current actor state.
    fn handle_received_frame(&mut self, data: Vec<u8>, ctx: &mut Context<Self>) {
        match &self.state {
            ActorState::Handshaking => {
                self.handle_handshake_frame(data, ctx);
            }
            ActorState::Ready => {
                // After handshake, waiting for routes to be set
                warn!(
                    "Received frame in Ready state before routes set, buffering not implemented - frame dropped"
                );
            }
            ActorState::Proxy { routes } => {
                self.handle_proxy_frame(data, routes.clone(), ctx);
            }
            ActorState::Failed => {
                warn!("Received frame in Failed state, ignoring");
            }
        }
    }

    /// Processes frames in Proxy state - forwards to appropriate room.
    fn handle_proxy_frame(
        &mut self,
        data: Vec<u8>,
        routes: HashMap<RoomId, Recipient<InboundRoomPayload>>,
        ctx: &mut Context<Self>,
    ) {
        match Frame::deserialize(&data) {
            Ok(Frame::Room(RoomFrame::Message {
                to_room,
                from_room,
                payload,
            })) => {
                debug!(
                    "Proxying message: {} -> {}, {} bytes",
                    from_room,
                    to_room,
                    payload.len()
                );
                if let Some(room_recipient) = routes.get(&RoomId::from(to_room.as_str())) {
                    room_recipient.do_send(InboundRoomPayload { payload });
                } else {
                    warn!("No route found for room: {}", to_room);
                }
            }
            Ok(Frame::Room(RoomFrame::Disconnect)) => {
                info!("Received disconnect frame, stopping actor");
                ctx.stop();
            }
            Ok(other_frame) => {
                warn!(
                    "Received unexpected frame type in Proxy state: {:?}",
                    other_frame
                );
            }
            Err(e) => {
                error!("Failed to deserialize frame in Proxy state: {:?}", e);
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

    fn complete_handshake(&mut self, ctx: &mut Context<Self>) {
        if let Some(rooms) = self.handshake.active_rooms() {
            info!("Handshake complete! Active rooms: {:?}", rooms);
            self.active_rooms = rooms.to_vec();
            self.state = ActorState::Ready;

            // Notify handshake recipient if configured
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
                            "Notified handshake recipient of handshake completion for peer {}",
                            peer_hostname
                        );
                    } else {
                        warn!("Handshake complete but peer_hostname not available");
                    }
                } else {
                    warn!("Handshake complete but peer_role not available");
                }
            } else {
                debug!("No handshake recipient configured, running standalone");
            }
        } else {
            error!("Handshake marked complete but no active rooms");
            self.state = ActorState::Failed;
        }
    }

    /// Logs an error, sends an error frame to the peer, and stops the actor.
    fn handle_error(&mut self, error: HelloError, ctx: &mut Context<Self>) {
        error!("HelloActor fatal error: {}", error);
        self.state = ActorState::Failed;

        // Try to send error frame to peer
        if let Ok(error_frame) = Frame::Handshake(zznet_api::HandshakeFrame::Error {
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

impl StreamHandler<Result<TransportFrame, TransportError>> for HelloActor {
    fn handle(&mut self, item: Result<TransportFrame, TransportError>, ctx: &mut Context<Self>) {
        match item {
            Ok(frame) => self.handle_received_frame(frame.get_bytes().to_vec(), ctx),
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

impl Handler<SetRoutes> for HelloActor {
    type Result = ();

    fn handle(&mut self, msg: SetRoutes, _ctx: &mut Context<Self>) -> Self::Result {
        if self.state != ActorState::Ready {
            error!(
                "SetRoutes called in wrong state: {:?}, expected Ready",
                self.state
            );
            return;
        }

        info!("Transitioning to Proxy state with {} routes", msg.0.len());
        self.state = ActorState::Proxy { routes: msg.0 };
    }
}

impl Handler<GetTransportTx> for HelloActor {
    type Result = MessageResult<GetTransportTx>;

    fn handle(&mut self, _msg: GetTransportTx, _ctx: &mut Context<Self>) -> Self::Result {
        MessageResult(self.transport_tx.clone())
    }
}

/// Starts a HelloActor that will report handshake completion to the provided recipient (usually ConnectionManager).
pub(crate) fn start_hello_actor_with_handshake_recipient(
    transport: Box<dyn TransportConnection>,
    config: HelloConfig,
    handshake_recipient: Option<Recipient<HandshakeComplete>>,
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
    if let Some(sm) = handshake_recipient {
        actor = actor.with_session_manager(sm);
    }

    actor.start()
}
