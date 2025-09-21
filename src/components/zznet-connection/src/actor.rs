//! Contains the implementation of the ephemeral ZzNetConnActor.
//!
//! This actor manages the `zznet` protocol for a single, underlying transport connection.
//! Its lifetime is tied to that connection.

use crate::auth::AuthRole;
use crate::bus::{DataForRoom, RoomIsActive, RoomSubscribers, RoomTerminated, SendDataToRoom};
use crate::protocol::{Frame, Handshake, deserialize, serialize};
use actix::prelude::*;
use std::collections::HashMap;

pub struct ZzNetConnActor {
    /// A handle to the underlying transport actor for this connection.
    transport: Recipient<FrameForTransport>,
    /// The current state of the handshake protocol.
    handshake: Handshake,
    /// A map of Room Names to the subscribers interested in them.
    /// This is provided by the manager when the actor is created.
    subscribers: HashMap<String, RoomSubscribers>,
    /// The current state of the actor.
    state: ConnActorState,
    /// Protocol version to use.
    protocol_version: String,
    /// Auth role.
    auth_role: AuthRole,
    /// Offered rooms.
    offered_rooms: Vec<String>,
    /// Active rooms after negotiation.
    active_rooms: Vec<String>,
    /// Manager to notify on termination.
    manager: Recipient<ConnectionTerminated>,
}

/// A message sent from the transport layer TO this actor with an incoming frame.
#[derive(Message)]
#[rtype(result = "()")]
pub struct FrameFromTransport(pub Vec<u8>);

/// A message sent TO this actor from a Room, containing a frame to be sent out.
#[derive(Message)]
#[rtype(result = "()")]
pub struct FrameForTransport(pub Vec<u8>);

/// The notification sent from the transport layer to a `ZzNetConnActor`
/// when a new connection is available.
#[derive(Message, Debug)]
#[rtype(result = "()")]
pub struct NewTransportConnection {
    /// A handle to the newly created transport connection actor. The consumer
    /// will use this to send outgoing frames.
    pub transport_handle: Recipient<FrameForTransport>,
    // In a real transport, you might also include the remote peer's address.
    // pub peer_addr: std::net::SocketAddr,
}

/// A message sent from the transport to indicate termination.
#[derive(Message)]
#[rtype(result = "()")]
pub struct TransportTerminated;

/// A message sent to the manager when the connection terminates.
#[derive(Message)]
#[rtype(result = "()")]
pub struct ConnectionTerminated {
    pub connection_actor: Addr<ZzNetConnActor>,
}

/// A message sent from the manager to the actor to re-publish rooms.
#[derive(Message)]
#[rtype(result = "()")]
pub struct RepublishRooms {
    pub subscribers: HashMap<String, RoomSubscribers>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConnActorState {
    AwaitingHandshake,
    AwaitingRooms,
    Active,
}

impl ZzNetConnActor {
    pub fn new(
        transport: Recipient<FrameForTransport>,
        subscribers: HashMap<String, RoomSubscribers>,
        protocol_version: String,
        auth_role: AuthRole,
        offered_rooms: Vec<String>,
        manager: Recipient<ConnectionTerminated>,
    ) -> Self {
        Self {
            transport,
            handshake: Handshake::new(), // The handshake starts in an initial state
            subscribers,
            state: ConnActorState::AwaitingHandshake,
            protocol_version,
            auth_role,
            offered_rooms: offered_rooms.clone(),
            active_rooms: Vec::new(),
            manager,
        }
    }
}

impl Actor for ZzNetConnActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        log::info!("ZzNetConnActor started. Beginning handshake.");
        // In a real implementation, we might need to tell the transport
        // that we are ready to receive frames. For now, we assume it starts sending.

        // Kick off the handshake by sending the first Hello message.
        let hello_frame = match self.handshake.create_hello_frame(
            self.protocol_version.clone(),
            self.auth_role.clone(),
            self.offered_rooms.clone(),
        ) {
            Ok(frame) => frame,
            Err(e) => {
                log::error!("Failed to create hello frame: {}", e);
                ctx.stop();
                return;
            }
        };
        self.transport.do_send(FrameForTransport(hello_frame));
    }

    fn stopped(&mut self, ctx: &mut Context<Self>) {
        log::info!("ZzNetConnActor stopped.");
        // Send termination notification to manager
        self.manager.do_send(ConnectionTerminated {
            connection_actor: ctx.address(),
        });
    }
}

/// Handles raw frames coming UP from the transport layer.
impl Handler<FrameFromTransport> for ZzNetConnActor {
    type Result = ();

    fn handle(&mut self, msg: FrameFromTransport, ctx: &mut Context<Self>) {
        log::debug!("ZzNetConnActor received a frame from transport.");

        match self.state {
            ConnActorState::AwaitingHandshake => {
                if !self.handshake.is_complete() {
                    // The handshake logic will mutate its own state and may produce an outgoing frame.
                    let response_frame_option = self.handshake.process_frame(msg.0);
                    match response_frame_option {
                        Ok(Some(response_frame)) => {
                            self.transport.do_send(FrameForTransport(response_frame));
                        }
                        Ok(None) => {}
                        Err(e) => {
                            log::error!("Handshake error: {}", e);
                            ctx.stop();
                            return;
                        }
                    }

                    // If the handshake just completed, send PublishRooms and transition.
                    if self.handshake.is_complete() {
                        log::info!("Handshake complete. Sending PublishRooms.");
                        let publish_frame = Frame::Room(crate::protocol::RoomFrame::PublishRooms {
                            offered_rooms: self.offered_rooms.clone(),
                        });
                        let serialized = match serialize(&publish_frame) {
                            Ok(s) => s,
                            Err(e) => {
                                log::error!("Failed to serialize publish frame: {}", e);
                                ctx.stop();
                                return;
                            }
                        };
                        self.transport.do_send(FrameForTransport(serialized));
                        self.state = ConnActorState::AwaitingRooms;
                    }
                }
            }
            ConnActorState::AwaitingRooms => {
                let frame = match deserialize(&msg.0) {
                    Ok(f) => f,
                    Err(e) => {
                        log::error!("Failed to deserialize frame: {}", e);
                        ctx.stop();
                        return;
                    }
                };
                match frame {
                    Frame::Room(crate::protocol::RoomFrame::PublishRooms {
                        offered_rooms: peer_offered,
                    }) => {
                        let intersection: Vec<String> = self
                            .offered_rooms
                            .iter()
                            .filter(|&room| peer_offered.contains(room))
                            .cloned()
                            .collect();
                        self.active_rooms = intersection.clone();
                        log::info!("Active rooms: {:?}", intersection);
                        for room_name in intersection {
                            if let Some(subscriber) = self.subscribers.get(&room_name) {
                                subscriber.room_is_active.do_send(RoomIsActive {
                                    room_name: room_name.clone(),
                                    connection_actor: ctx.address(),
                                });
                            }
                        }
                        self.state = ConnActorState::Active;
                    }
                    _ => {
                        log::error!("Expected PublishRooms frame in AwaitingRooms state");
                        ctx.stop();
                    }
                }
            }
            ConnActorState::Active => {
                let frame = match deserialize(&msg.0) {
                    Ok(f) => f,
                    Err(e) => {
                        log::error!("Failed to deserialize frame: {}", e);
                        return;
                    }
                };
                match frame {
                    Frame::Room(crate::protocol::RoomFrame::MessageForRoom { room, data }) => {
                        if let Some(subscriber) = self.subscribers.get(&room) {
                            log::debug!("Forwarding data for room {}: {} bytes", room, data.len());
                            subscriber.data.do_send(DataForRoom {
                                room_name: room,
                                data,
                            });
                        }
                    }
                    _ => {
                        log::warn!("Unexpected frame in Active state: {:?}", frame);
                    }
                }
            }
        }
    }
}

/// Handles frames coming DOWN from Room actors, to be sent to the transport.
impl Handler<FrameForTransport> for ZzNetConnActor {
    type Result = ();

    fn handle(&mut self, msg: FrameForTransport, _ctx: &mut Context<Self>) {
        if self.state == ConnActorState::Active {
            log::debug!("ZzNetConnActor sending frame to transport.");
            self.transport.do_send(msg);
        } else {
            log::warn!("FrameForTransport received before Active state. Frame dropped.");
        }
    }
}

/// Handles new transport connection notification.
impl Handler<NewTransportConnection> for ZzNetConnActor {
    type Result = ();

    fn handle(&mut self, msg: NewTransportConnection, _ctx: &mut Context<Self>) {
        log::debug!("ZzNetConnActor received new transport connection.");
        self.transport = msg.transport_handle;
        // If not yet started handshake, send hello now.
        if self.state == ConnActorState::AwaitingHandshake {
            let hello_frame = match self.handshake.create_hello_frame(
                self.protocol_version.clone(),
                self.auth_role.clone(),
                self.offered_rooms.clone(),
            ) {
                Ok(frame) => frame,
                Err(e) => {
                    log::error!("Failed to create hello frame: {}", e);
                    return;
                }
            };
            self.transport.do_send(FrameForTransport(hello_frame));
        }
    }
}

/// Handles transport termination.
impl Handler<TransportTerminated> for ZzNetConnActor {
    type Result = ();

    fn handle(&mut self, _msg: TransportTerminated, ctx: &mut Context<Self>) {
        log::info!("Transport terminated. Shutting down connection actor.");
        // Notify all active rooms.
        for room_name in &self.active_rooms {
            log::info!("Notifying room {} of termination", room_name);
            if let Some(subscriber) = self.subscribers.get(room_name) {
                subscriber.termination.do_send(RoomTerminated {
                    room_name: room_name.clone(),
                });
            }
        }
        // Notify the manager.
        self.manager.do_send(ConnectionTerminated {
            connection_actor: ctx.address(),
        });
        ctx.stop();
    }
}

/// Handles re-publish rooms command from manager.
impl Handler<RepublishRooms> for ZzNetConnActor {
    type Result = ();

    fn handle(&mut self, msg: RepublishRooms, _ctx: &mut Context<Self>) {
        // Update subscribers
        self.subscribers = msg.subscribers;
        // Send RoomIsActive for all active rooms to the new subscribers
        for room_name in &self.active_rooms {
            if let Some(subscriber) = self.subscribers.get(room_name) {
                subscriber.room_is_active.do_send(RoomIsActive {
                    room_name: room_name.clone(),
                    connection_actor: _ctx.address(),
                });
            }
        }
        if self.handshake.is_complete() {
            log::info!("Re-publishing rooms.");
            let publish_frame = Frame::Room(crate::protocol::RoomFrame::PublishRooms {
                offered_rooms: self.offered_rooms.clone(),
            });
            let serialized = match serialize(&publish_frame) {
                Ok(s) => s,
                Err(e) => {
                    log::error!("Failed to serialize republish frame: {}", e);
                    return;
                }
            };
            self.transport.do_send(FrameForTransport(serialized));
        }
    }
}

/// Handles data sending requests from room actors.
impl Handler<SendDataToRoom> for ZzNetConnActor {
    type Result = ();

    fn handle(&mut self, msg: SendDataToRoom, _ctx: &mut Context<Self>) {
        if self.state == ConnActorState::Active && self.active_rooms.contains(&msg.room_name) {
            log::debug!(
                "Sending data to room {}: {} bytes",
                msg.room_name,
                msg.data.len()
            );
            let frame = Frame::Room(crate::protocol::RoomFrame::MessageForRoom {
                room: msg.room_name,
                data: msg.data,
            });
            let serialized = match serialize(&frame) {
                Ok(s) => s,
                Err(e) => {
                    log::error!("Failed to serialize message frame: {}", e);
                    return;
                }
            };
            self.transport.do_send(FrameForTransport(serialized));
        } else {
            log::warn!("Attempted to send data to inactive room: {}", msg.room_name);
        }
    }
}
