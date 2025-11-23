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

/// Message to set the room_actor address after NetworkActor creation
///
/// Used to resolve circular dependency in factory.
/// Factory creates NetworkActor first, then RoomActor, then wires them together.
#[derive(Clone)]
pub struct SetRoomActor(pub Addr<zznet_room::RoomActor<ComponentAMessage>>);

impl Message for SetRoomActor {
    type Result = ();
}

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
        if let Some(room_actor) = &self.room_actor {
            room_actor.do_send(ComponentAMessage::Ping((msg.0, msg.1)));
        }
    }
}

impl Handler<SendPongToRoom> for ComponentANetworkActor {
    type Result = ();

    fn handle(&mut self, msg: SendPongToRoom, _ctx: &mut Self::Context) {
        if let Some(room_actor) = &self.room_actor {
            room_actor.do_send(ComponentAMessage::Pong((msg.0, msg.1)));
        }
    }
}

use crate::messages::{
    ComponentAMessage, GetCounter, PublishToA, SendPing, SetNetworkManager, StateUpdate, Subscribe,
};
use actix::prelude::*;
use std::collections::HashMap;
use tracing::info;
use zznet_api::PeerId;
use zznet_router::RegisterManager;

/// Custom factory for ComponentA that wires NetworkActor with RoomActor
///
/// Replaces StandardRoomFactory to properly wire NetworkActor with RoomActor via SetRoomActor.
pub struct ComponentARoomFactory {
    main_actor: Addr<ComponentAActor>,
    permissions_map: HashMap<String, ComponentAPermissions>,
}

impl ComponentARoomFactory {
    /// Create a new ComponentARoomFactory
    pub fn new(
        _manager: Addr<ComponentANetworkManager>,
        main_actor: Addr<ComponentAActor>,
        permissions_map: HashMap<String, ComponentAPermissions>,
    ) -> Self {
        Self {
            main_actor,
            permissions_map,
        }
    }
}

impl zznet_router::RoomFactory for ComponentARoomFactory {
    fn create_room(
        &self,
        peer_id: PeerId,
        role: zznet_api::Role,
        room_id: zznet_api::RoomId,
        transport_tx: tokio::sync::mpsc::Sender<zznet_api::TransportFrame>,
    ) -> Result<Option<zznet_room::RoomInboundRecipient>, String> {
        // Check if this is our room
        if room_id.as_str() != "room-a" {
            return Ok(None);
        }

        tracing::debug!(
            "Creating room for peer {} with role {}",
            peer_id,
            role.as_str()
        );

        // Lookup permissions
        let perms = self
            .permissions_map
            .get(role.as_str())
            .cloned()
            .unwrap_or_default();

        // Create NetworkActor
        let net = ComponentANetworkActor::new(peer_id.clone(), perms, self.main_actor.clone());
        let net_addr = net.start();

        // Create RoomActor
        let room = zznet_room::RoomActor::new(
            room_id,
            transport_tx,
            net_addr.clone().recipient::<ComponentAMessage>(),
        );
        let room_addr = room.start();

        // Wire them together via SetRoomActor message
        net_addr.do_send(SetRoomActor(room_addr.clone()));

        Ok(Some(room_addr.recipient()))
    }
}

/// MainActor for ComponentA - handles business logic and local subscriptions.
#[derive(Debug)]
pub struct ComponentAActor {
    /// Current counter value
    counter: u64,
    /// Some arbitrary data
    data: String,
    /// Local subscribers to state updates
    subscribers: Vec<Recipient<StateUpdate>>,
    /// NetworkManager for sending network messages
    network_manager: Option<Addr<ComponentANetworkManager>>,
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
            network_manager: None,
            event_tx,
        }
    }

    /// Set the network manager address
    pub fn set_network_manager(&mut self, addr: Addr<ComponentANetworkManager>) {
        self.network_manager = Some(addr);
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

impl Handler<SetNetworkManager> for ComponentAActor {
    type Result = ();

    fn handle(&mut self, msg: SetNetworkManager, _ctx: &mut Self::Context) -> Self::Result {
        self.set_network_manager(msg.network_manager);
    }
}

/// NetworkManager for ComponentA - orchestrates peer lifecycle and message routing.
#[derive(Clone)]
pub struct ComponentANetworkManager {
    /// Address of the main actor
    main_actor: Addr<ComponentAActor>,
    /// Router for room registration
    router: Addr<zznet_router::RouterActor>,
    /// Policy map from role strings to component-specific permissions
    permissions_map: HashMap<String, ComponentAPermissions>,
}

impl ComponentANetworkManager {
    /// Create a new NetworkManager
    pub fn new(
        main_actor: Addr<ComponentAActor>,
        router: Addr<zznet_router::RouterActor>,
        permissions_map: HashMap<String, ComponentAPermissions>,
    ) -> Self {
        Self {
            main_actor,
            router,
            permissions_map,
        }
    }
}

impl Actor for ComponentANetworkManager {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        tracing::debug!("ComponentANetworkManager started");

        // Register with router using the custom ComponentARoomFactory
        let factory = std::sync::Arc::new(ComponentARoomFactory::new(
            ctx.address(),
            self.main_actor.clone(),
            self.permissions_map.clone(),
        ));
        let rooms = vec![zznet_api::RoomId::from("room-a")];
        let register_msg = RegisterManager { factory, rooms };
        self.router.do_send(register_msg);
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        tracing::debug!("ComponentANetworkManager stopped");
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
    room_actor: Option<Addr<zznet_room::RoomActor<ComponentAMessage>>>,
}

impl ComponentANetworkActor {
    /// Create a new NetworkActor for a specific peer
    pub fn new(
        peer_id: PeerId,
        permissions: ComponentAPermissions,
        main_actor: Addr<ComponentAActor>,
    ) -> Self {
        Self {
            peer_id,
            permissions,
            main_actor,
            room_actor: None,
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

        // Subscribe to events from main actor
        let main_actor = self.main_actor.clone();
        let addr = ctx.address();
        actix::spawn(async move {
            if let Ok(event_tx) = main_actor.send(crate::messages::GetEventBus).await {
                let mut event_rx = event_tx.subscribe();
                while let Ok(event) = event_rx.recv().await {
                    match event {
                        crate::messages::ComponentAEvent::StateChanged { counter, data } => {
                            // Send message to self to send to room
                            let _ = addr.send(SendPingToRoom(counter, data)).await;
                        }
                        crate::messages::ComponentAEvent::Pong { counter, data } => {
                            // Send Pong to room
                            let _ = addr.send(SendPongToRoom(counter, data)).await;
                        }
                    }
                }
            }
        });
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        tracing::debug!(
            "ComponentANetworkActor stopped for peer: {:?}",
            self.peer_id
        );
    }
}

impl Handler<SetRoomActor> for ComponentANetworkActor {
    type Result = ();

    fn handle(&mut self, msg: SetRoomActor, _ctx: &mut Self::Context) {
        self.room_actor = Some(msg.0);
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
