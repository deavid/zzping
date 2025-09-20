//! Contains the implementation of the long-lived ZzNetConnManager actor.
//!
//! This actor is the central hub for the connection layer. It accepts subscriptions
//! from RoomManagers and spawns ephemeral ZzNetConnActors for each new
//! underlying transport connection.

use crate::actor::ConnectionTerminated;
use crate::actor::DummyTransportActor;
use crate::actor::ZzNetConnActor;
use crate::bus::{RoomIsActive, SubscribeToRoom, UnsubscribeFromRoom};
use actix::prelude::*;
use std::collections::HashMap;
use std::collections::HashSet;

// --- Placeholders for dependencies from other crates ---

// This message would come from the `zznet-transport` crate's public API.
// It signals that a new, raw, framed connection is available.
#[derive(Message)]
#[rtype(result = "()")]
pub struct NewTransportConnection {
    // This handle would also be defined in the transport crate.
    pub transport_handle: Addr<DummyTransportActor>,
}

/// A message to register an offered room.
#[derive(Message)]
#[rtype(result = "()")]
pub struct RegisterOfferedRoom(pub String);

// --- The Manager Actor Implementation ---

#[derive(Default)]
pub struct ZzNetConnManager {
    /// Maps a room name to the list of subscribers interested in it.
    /// For now, we'll simplify and assume one subscriber per room. A real
    /// implementation might use a Vec<Recipient<...>>.
    subscribers: HashMap<String, Recipient<RoomIsActive>>,
    /// The set of rooms we are offering.
    offered_rooms: HashSet<String>,
    /// The active connection actors.
    active_connections: Vec<Addr<ZzNetConnActor>>,
}

// --- The Manager Actor Implementation ---

impl ZzNetConnManager {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Actor for ZzNetConnManager {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Context<Self>) {
        log::info!("ZzNetConnManager has started.");
        // In a real system, it might subscribe to a TransportManager here,
        // but in our decoupled design, the Composer wires them together.
    }
}

/// Handles subscription requests from RoomManagers.
impl Handler<SubscribeToRoom> for ZzNetConnManager {
    type Result = ();

    fn handle(&mut self, msg: SubscribeToRoom, _ctx: &mut Context<Self>) {
        log::info!("Manager received subscription for room '{}'", msg.room_name);
        self.subscribers
            .insert(msg.room_name.clone(), msg.subscriber);
        self.offered_rooms.insert(msg.room_name);
    }
}

/// Handles unsubscription requests.
impl Handler<UnsubscribeFromRoom> for ZzNetConnManager {
    type Result = ();

    fn handle(&mut self, msg: UnsubscribeFromRoom, _ctx: &mut Context<Self>) {
        log::info!("Manager received unsubscribe for room '{}'", msg.room_name);
        self.subscribers.remove(&msg.room_name);
    }
}

/// Handles notifications of new transport connections.
/// This is the primary trigger for the manager's orchestration logic.
impl Handler<NewTransportConnection> for ZzNetConnManager {
    type Result = ();

    fn handle(&mut self, msg: NewTransportConnection, ctx: &mut Context<Self>) {
        log::info!("Manager received new transport. Spawning ZzNetConnActor.");

        // When a new connection is available, we spawn a new ZzNetConnActor
        // to manage it. We give the new actor a clone of the current
        // subscriber list so it knows who to notify when handshakes complete.
        let actor = ZzNetConnActor::new(
            msg.transport_handle,
            self.subscribers.clone(),
            "1.0".to_string(),
            "client".to_string(),
            self.offered_rooms.iter().cloned().collect(),
            ctx.address().recipient(),
        )
        .start();
        self.active_connections.push(actor);
    }
}

impl Handler<ConnectionTerminated> for ZzNetConnManager {
    type Result = ();

    fn handle(&mut self, msg: ConnectionTerminated, _ctx: &mut Context<Self>) {
        log::info!("Connection terminated: {:?}", msg.connection_actor);
        self.active_connections
            .retain(|actor| actor != &msg.connection_actor);
        // TODO: Clean up any state related to this connection.
    }
}

impl Handler<RegisterOfferedRoom> for ZzNetConnManager {
    type Result = ();

    fn handle(&mut self, msg: RegisterOfferedRoom, _ctx: &mut Context<Self>) {
        log::info!("Registering offered room '{}'", msg.0);
        self.offered_rooms.insert(msg.0);
        // Trigger re-publication to all active connections.
        for actor in &self.active_connections {
            actor.do_send(crate::actor::RepublishRooms);
        }
    }
}
