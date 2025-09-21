//! Contains the implementation of the long-lived ZzNetConnManager actor.
//!
//! This actor is the central hub for the connection layer. It accepts subscriptions
//! from RoomManagers and spawns ephemeral ZzNetConnActors for each new
//! underlying transport connection.

use crate::actor::ZzNetConnActor;
use crate::actor::{ConnectionTerminated, FrameForTransport, RepublishRooms};
use crate::auth::AuthRole;
use crate::bus::{RoomSubscribers, RoomTerminated, SubscribeToRoom, UnsubscribeFromRoom};
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
    pub transport_handle: Recipient<FrameForTransport>,
}

/// A message to register an offered room.
#[derive(Message)]
#[rtype(result = "()")]
pub struct RegisterOfferedRoom(pub String);

// --- Subscriber Info ---

// Moved to bus.rs

// --- The Manager Actor Implementation ---

#[derive(Default)]
pub struct ZzNetConnManager {
    /// Maps a room name to the subscribers interested in it.
    subscribers: HashMap<String, RoomSubscribers>,
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
        let room_subs = RoomSubscribers {
            room_is_active: msg.room_is_active_recipient,
            data: msg.data_recipient,
            termination: msg.termination_recipient,
        };
        self.subscribers.insert(msg.room_name.clone(), room_subs);
        self.offered_rooms.insert(msg.room_name);
        // Trigger re-publication to all active connections.
        for actor in &self.active_connections {
            actor.do_send(crate::actor::RepublishRooms {
                subscribers: self.subscribers.clone(),
            });
        }
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
            AuthRole::Collector,
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

        // 1. Remove from active connections
        let was_removed = self
            .active_connections
            .iter()
            .position(|actor| actor == &msg.connection_actor)
            .map(|pos| self.active_connections.remove(pos))
            .is_some();

        if was_removed {
            // 2. Notify subscribers about connection loss for active rooms
            for (room_name, subscribers) in &self.subscribers {
                if self.offered_rooms.contains(room_name) {
                    subscribers.termination.do_send(RoomTerminated {
                        room_name: room_name.clone(),
                    });
                }
            }

            // 3. Re-publish rooms to remaining connections to maintain service
            for actor in &self.active_connections {
                actor.do_send(RepublishRooms {
                    subscribers: self.subscribers.clone(),
                });
            }

            // 4. Log connection count for monitoring
            log::info!(
                "Active connections after cleanup: {}",
                self.active_connections.len()
            );
        }
    }
}

impl Handler<RegisterOfferedRoom> for ZzNetConnManager {
    type Result = ();

    fn handle(&mut self, msg: RegisterOfferedRoom, _ctx: &mut Context<Self>) {
        log::info!("Registering offered room '{}'", msg.0);
        self.offered_rooms.insert(msg.0);
        // Trigger re-publication to all active connections.
        for actor in &self.active_connections {
            actor.do_send(crate::actor::RepublishRooms {
                subscribers: self.subscribers.clone(),
            });
        }
    }
}
