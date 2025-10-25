//! Simple test component to validate Room<T> auto-registration pattern

use actix::prelude::*;
use serde::{Deserialize, Serialize};
use zznet_room::room::Room;

/// Simple message type for testing
#[derive(Debug, Clone, Serialize, Deserialize, Message)]
#[rtype(result = "()")]
pub struct TestMessage {
    pub content: String,
    pub sequence: u32,
}

/// Simple actor that uses Room<T> for network communication
pub struct SimpleActor {
    pub name: String,
    pub room: Option<Room<TestMessage>>,
    pub messages_received: Vec<TestMessage>,
}

impl SimpleActor {
    pub fn new(name: String) -> Self {
        Self {
            name,
            room: None,
            messages_received: Vec::new(),
        }
    }

    pub fn with_room(mut self, room: Room<TestMessage>) -> Self {
        self.room = Some(room);
        self
    }
}

impl Actor for SimpleActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        tracing::info!("{}: Actor started", self.name);

        // Spawn receiver task for the room if present
        if let Some(room) = &mut self.room {
            if let Err(e) = room.spawn_receiver() {
                tracing::error!("{}: Failed to spawn receiver: {:?}", self.name, e);
            } else {
                tracing::info!("{}: Room receiver spawned", self.name);
            }
        }
    }
}

/// Handle incoming test messages
impl Handler<TestMessage> for SimpleActor {
    type Result = ();

    fn handle(&mut self, msg: TestMessage, _ctx: &mut Self::Context) -> Self::Result {
        tracing::info!(
            "{}: Received message #{}: {}",
            self.name,
            msg.sequence,
            msg.content
        );
        self.messages_received.push(msg);
    }
}

/// Builder pattern for SimpleActor
pub struct SimpleActorBuilder {
    name: String,
    with_room: bool,
}

impl SimpleActorBuilder {
    pub fn new(name: String) -> Self {
        Self {
            name,
            with_room: false,
        }
    }

    /// This is the key pattern we're testing: with_session_manager()
    ///
    /// In the real implementation (Phase 1), this would accept SessionManager
    /// and auto-register the Room. For now, we just flag that we want a room.
    pub fn with_session_manager(mut self) -> Self {
        self.with_room = true;
        self
    }

    /// Start the actor with auto-registered Room<T>
    pub fn start(self) -> Result<Addr<SimpleActor>, String> {
        let actor = SimpleActor::new(self.name.clone());
        let actor_addr = actor.start();

        // If we want a room, create one
        if self.with_room {
            tracing::info!("{}: Creating Room<TestMessage>", self.name);

            let (room, _channels) = Room::new(
                format!("test-room-{}", self.name),
                actor_addr.clone().recipient(),
            );

            // TODO: In Phase 1, this will be:
            // let room = Room::new_with_session_manager(
            //     format!("test-room-{}", self.name),
            //     actor_addr.clone().recipient(),
            //     session_manager,  // <- Would auto-register here
            // );
            // And we wouldn't need the channels, Room would handle them internally

            actor_addr.do_send(SetRoom(room));

            tracing::info!("{}: Room created", self.name);
            tracing::info!("NOTE: Manual channel wiring would go here");
            tracing::info!("      Phase 1 will eliminate this by implementing auto-registration");
        }

        Ok(actor_addr)
    }
}

/// Message to set the room on an actor
#[derive(Message)]
#[rtype(result = "()")]
struct SetRoom(Room<TestMessage>);

impl Handler<SetRoom> for SimpleActor {
    type Result = ();

    fn handle(&mut self, msg: SetRoom, _ctx: &mut Self::Context) -> Self::Result {
        self.room = Some(msg.0);

        // Spawn receiver after setting room
        if let Some(room) = &mut self.room {
            if let Err(e) = room.spawn_receiver() {
                tracing::error!("{}: Failed to spawn receiver: {:?}", self.name, e);
            }
        }
    }
}

/// Message to send via room
#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct SendViaRoom(pub TestMessage);

impl Handler<SendViaRoom> for SimpleActor {
    type Result = ResponseFuture<Result<(), String>>;

    fn handle(&mut self, msg: SendViaRoom, _ctx: &mut Self::Context) -> Self::Result {
        let sender = match &self.room {
            Some(r) => r.typed_sender(),
            None => return Box::pin(async { Err("No room configured".to_string()) }),
        };

        let name = self.name.clone();

        Box::pin(async move {
            tracing::info!("{}: Sending message via room: {:?}", name, msg.0);

            // Send typed message via room's typed sender
            sender
                .send(msg.0)
                .await
                .map_err(|e| format!("Send failed: {:?}", e))
        })
    }
}

/// Message to get received messages count
#[derive(Message)]
#[rtype(result = "usize")]
pub struct GetReceivedCount;

impl Handler<GetReceivedCount> for SimpleActor {
    type Result = usize;

    fn handle(&mut self, _msg: GetReceivedCount, _ctx: &mut Self::Context) -> Self::Result {
        self.messages_received.len()
    }
}
