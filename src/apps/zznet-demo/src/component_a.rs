//! ComponentA implementation following the three-actor pattern.
//!
//! ComponentA demonstrates network communication and local pub/sub.
//! It maintains a counter that can be updated via network messages and
//! broadcasts state changes to local subscribers.

/// ComponentA Permissions
///
/// Defines the permissions structure for ComponentA.
pub mod permissions {
    /// Permissions for ComponentA
    ///
    /// This struct defines what a peer can do within ComponentA's room.
    /// Permissions are granted by the application based on the peer's global Role,
    /// but the component itself is role-agnostic and only enforces these local permissions.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct ComponentAPermissions {
        /// Whether the peer can send ping messages
        pub can_ping: bool,
        /// Whether the peer can publish messages to ComponentA
        pub can_publish: bool,
    }

    impl ComponentAPermissions {
        /// Create a new permissions struct with the given capabilities
        pub fn new(can_ping: bool, can_publish: bool) -> Self {
            Self {
                can_ping,
                can_publish,
            }
        }

        /// Permissions that allow full access (ping and publish)
        pub fn full_access() -> Self {
            Self::new(true, true)
        }

        /// Permissions that allow only pinging
        pub fn ping_only() -> Self {
            Self::new(true, false)
        }

        /// Permissions that deny all access
        pub fn no_access() -> Self {
            Self::new(false, false)
        }
    }
}

pub use permissions::ComponentAPermissions;

/// Internal message to send ping to room
#[derive(Message)]
#[rtype(result = "()")]
struct SendPingToRoom(u64, String);

/// Internal message to send pong to room
#[derive(Message)]
#[rtype(result = "()")]
struct SendPongToRoom(u64, String);

impl Handler<SendPingToRoom> for ComponentANetworkActor {
    type Result = ();

    fn handle(&mut self, msg: SendPingToRoom, _ctx: &mut Self::Context) {
        self.room_actor
            .do_send(ComponentAMessage::Ping((msg.0, msg.1)));
    }
}

impl Handler<SendPongToRoom> for ComponentANetworkActor {
    type Result = ();

    fn handle(&mut self, msg: SendPongToRoom, _ctx: &mut Self::Context) {
        self.room_actor
            .do_send(ComponentAMessage::Pong((msg.0, msg.1)));
    }
}

use crate::messages::{
    ComponentAMessage, GetCounter, PublishToA, SendPing, StateUpdate, Subscribe,
};
use actix::prelude::*;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tracing::info;
use zznet_api::PeerId;

/// MainActor for ComponentA - handles business logic and local subscriptions.
#[derive(Debug)]
pub struct ComponentAActor {
    /// Current counter value
    counter: u64,
    /// Some arbitrary data
    data: String,
    /// Local subscribers to state updates
    subscribers: Vec<Recipient<StateUpdate>>,
    /// Event bus sender for broadcasting state changes
    event_tx: tokio::sync::broadcast::Sender<crate::messages::ComponentAEvent>,
}

impl Default for ComponentAActor {
    fn default() -> Self {
        Self::new()
    }
}

impl ComponentAActor {
    /// Create a new ComponentAActor
    pub fn new() -> Self {
        let (event_tx, _) = tokio::sync::broadcast::channel(100);
        Self {
            counter: 0,
            data: String::new(),
            subscribers: Vec::new(),
            event_tx,
        }
    }

    /// Send a pong message over the network
    fn send_pong(&self, value: u64) -> Result<(), String> {
        // Send event for pong
        let _ = self.event_tx.send(crate::messages::ComponentAEvent::Pong {
            counter: value,
            data: "pong".to_string(),
        });
        Ok(())
    }

    /// Publish the current state to all subscribers
    fn publish_state(&self) {
        let state = StateUpdate {
            counter: self.counter,
            data: self.data.clone(),
        };
        for sub in &self.subscribers {
            sub.do_send(state.clone());
        }
        // Send event for network broadcasting
        let _ = self
            .event_tx
            .send(crate::messages::ComponentAEvent::StateChanged {
                counter: self.counter,
                data: self.data.clone(),
            });
    }
}

impl Actor for ComponentAActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        tracing::debug!("ComponentAActor started");
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        tracing::debug!("ComponentAActor stopped");
    }
}

/// Handle subscription requests
impl Handler<Subscribe> for ComponentAActor {
    type Result = ();

    fn handle(&mut self, msg: Subscribe, _ctx: &mut Self::Context) -> Self::Result {
        self.subscribers.push(msg.recipient);
        tracing::debug!(
            "Added subscriber, total subscribers: {}",
            self.subscribers.len()
        );
    }
}

/// Handle test ping trigger
impl Handler<SendPing> for ComponentAActor {
    type Result = ();

    fn handle(&mut self, msg: SendPing, _ctx: &mut Self::Context) {
        self.counter += 1;
        self.data = msg.data.clone();
        self.publish_state();
    }
}

/// Handle data publishing to ComponentA
impl Handler<PublishToA> for ComponentAActor {
    type Result = ();

    fn handle(&mut self, msg: PublishToA, _ctx: &mut Self::Context) {
        // This message now originates from ComponentB and is a request to publish.
        // We update state and broadcast via event bus.
        self.data = msg.data;
        self.counter += 1;
        self.publish_state();
    }
}

impl Handler<ComponentAMessage> for ComponentAActor {
    type Result = ();

    fn handle(&mut self, msg: ComponentAMessage, _ctx: &mut Self::Context) -> Self::Result {
        match msg {
            ComponentAMessage::Ping((value, data)) => {
                tracing::debug!("Received Ping({}, {}), updating counter", value, data);
                // When we receive a ping, we update our state and publish locally.
                self.counter = value;
                self.data = data;
                self.publish_state();
                // And respond with a Pong containing the *same* value.
                let _ = self.send_pong(value);
            }
            ComponentAMessage::Pong((value, data)) => {
                info!("Received Pong with value {} and data '{}'", value, data);
                // When we receive a Pong, it confirms the other side has our state.
                // We update our own state to match.
                self.counter = value;
                self.data = data;
                self.publish_state();
            }
        }
    }
}

impl Handler<GetCounter> for ComponentAActor {
    type Result = u64;

    fn handle(&mut self, _msg: GetCounter, _ctx: &mut Self::Context) -> Self::Result {
        self.counter
    }
}

impl Handler<crate::messages::GetEventBus> for ComponentAActor {
    type Result = MessageResult<crate::messages::GetEventBus>;

    fn handle(
        &mut self,
        _msg: crate::messages::GetEventBus,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        MessageResult(self.event_tx.clone())
    }
}

/// NetworkActor for ComponentA - handles per-peer protocol translation.
pub struct ComponentANetworkActor {
    /// Peer this actor handles
    peer_id: PeerId,
    /// Permissions for this peer
    permissions: ComponentAPermissions,
    /// Main actor address
    main_actor: Addr<ComponentAActor>,
    /// Room actor for outbound messages
    room_actor: Addr<zznet_room::RoomActor<ComponentAMessage>>,
    /// Event bus receiver for outbound events from MainActor
    event_rx: tokio::sync::broadcast::Receiver<crate::messages::ComponentAEvent>,
}

impl ComponentANetworkActor {
    /// Create a new NetworkActor for a specific peer
    pub fn new(
        peer_id: PeerId,
        permissions: ComponentAPermissions,
        main_actor: Addr<ComponentAActor>,
        event_rx: tokio::sync::broadcast::Receiver<crate::messages::ComponentAEvent>,
        room_actor: Addr<zznet_room::RoomActor<ComponentAMessage>>,
    ) -> Self {
        Self {
            peer_id,
            permissions,
            main_actor,
            room_actor,
            event_rx,
        }
    }
}

impl Actor for ComponentANetworkActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        tracing::debug!(
            "ComponentANetworkActor started for peer: {:?}",
            self.peer_id
        );

        // Subscribe to events from main actor using add_stream
        ctx.add_stream(tokio_stream::wrappers::BroadcastStream::new(
            self.event_rx.resubscribe(),
        ));
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        tracing::debug!(
            "ComponentANetworkActor stopped for peer: {:?}",
            self.peer_id
        );
    }
}

/// Handle events from the event bus
impl StreamHandler<Result<crate::messages::ComponentAEvent, BroadcastStreamRecvError>>
    for ComponentANetworkActor
{
    fn handle(
        &mut self,
        item: Result<crate::messages::ComponentAEvent, BroadcastStreamRecvError>,
        _ctx: &mut Context<Self>,
    ) {
        match item {
            Ok(crate::messages::ComponentAEvent::StateChanged { counter, data }) => {
                // Send Ping to room
                self.room_actor
                    .do_send(ComponentAMessage::Ping((counter, data)));
            }
            Ok(crate::messages::ComponentAEvent::Pong { counter, data }) => {
                // Send Pong to room
                self.room_actor
                    .do_send(ComponentAMessage::Pong((counter, data)));
            }
            Err(BroadcastStreamRecvError::Lagged(skipped)) => {
                tracing::warn!(
                    "Peer {:?} lagged on ComponentA event stream; skipped {} events",
                    self.peer_id,
                    skipped
                );
            }
        }
    }

    fn finished(&mut self, _ctx: &mut Context<Self>) {
        tracing::debug!("Event bus stream finished for peer {:?}", self.peer_id);
    }
}

/// Handle inbound messages from RoomActor (forward to MainActor)
impl Handler<ComponentAMessage> for ComponentANetworkActor {
    type Result = ();

    fn handle(&mut self, msg: ComponentAMessage, _ctx: &mut Self::Context) -> Self::Result {
        match msg {
            ComponentAMessage::Ping(_) => {
                // Check if peer can publish (send ping messages)
                if !self.permissions.can_publish {
                    tracing::warn!(
                        "Peer {:?} is not authorized to send ping messages (requires can_publish permission)",
                        self.peer_id
                    );
                    // TODO: Send error response if needed
                    return;
                }
            }
            ComponentAMessage::Pong(_) => {
                // Pong messages are responses, allow if peer can ping
                if !self.permissions.can_ping {
                    tracing::warn!(
                        "Peer {:?} is not authorized to send pong messages (requires can_ping permission)",
                        self.peer_id
                    );
                    return;
                }
            }
        }

        // Forward authorized message to main actor
        self.main_actor.do_send(msg);
    }
}
