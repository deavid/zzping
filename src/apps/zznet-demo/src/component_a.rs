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
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

use crate::messages::{
    ComponentAMessage, GetCounter, PublishToA, SendPing, SetNetworkManager, StateUpdate, Subscribe,
};
use actix::{Actor, Addr, Context, Handler, Recipient};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use tracing::info;
use zznet_api::types::{PeerId, Role, RoomId};
use zznet_room::actor::RoomActor;
use zznet_room::room_manager::{CreateError, RoomInboundRecipient, RoomManager};
use zznet_router::RegisterManager;

/// MainActor for ComponentA - handles business logic and local subscriptions.
#[derive(Debug, Default)]
pub struct ComponentAActor {
    /// Current counter value
    counter: u64,
    /// Some arbitrary data
    data: String,
    /// Local subscribers to state updates
    subscribers: Vec<Recipient<StateUpdate>>,
    /// NetworkManager for sending network messages
    network_manager: Option<Addr<ComponentANetworkManager>>,
}

impl ComponentAActor {
    /// Create a new ComponentAActor
    pub fn new() -> Self {
        Self {
            counter: 0,
            data: String::new(),
            subscribers: Vec::new(),
            network_manager: None,
        }
    }

    /// Set the network manager address
    pub fn set_network_manager(&mut self, addr: Addr<ComponentANetworkManager>) {
        self.network_manager = Some(addr);
    }

    /// Send a pong message over the network
    fn send_pong(&self, value: u64) -> Result<(), String> {
        if let Some(ref nm) = self.network_manager {
            nm.do_send(ComponentAMessage::Pong((value, "pong".to_string())));
            Ok(())
        } else {
            Err("NetworkManager not set".to_string())
        }
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
        if let Some(network_manager) = &self.network_manager {
            // We send a ComponentAMessage over the network, not the triggering SendPing message
            network_manager.do_send(ComponentAMessage::Ping((self.counter, self.data.clone())));
        }
        self.publish_state();
    }
}

/// Handle data publishing to ComponentA
impl Handler<PublishToA> for ComponentAActor {
    type Result = ();

    fn handle(&mut self, msg: PublishToA, _ctx: &mut Self::Context) {
        // This message now originates from ComponentB and is a request to publish.
        // We don't increment our own counter here, but use the state to send a Ping.
        self.data = msg.data;
        if let Some(nm) = &self.network_manager {
            nm.do_send(ComponentAMessage::Ping((
                self.counter + 1, // We send the *next* state
                self.data.clone(),
            )));
        }
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
    /// NetworkActors per peer
    network_actors:
        Arc<std::sync::RwLock<std::collections::HashMap<PeerId, Addr<ComponentANetworkActor>>>>,
    /// RoomActors per peer for outbound messaging
    room_actors: Arc<
        std::sync::RwLock<
            std::collections::HashMap<
                PeerId,
                Addr<zznet_room::actor::RoomActor<ComponentAMessage>>,
            >,
        >,
    >,
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
            network_actors: Arc::new(RwLock::new(std::collections::HashMap::new())),
            room_actors: Arc::new(RwLock::new(std::collections::HashMap::new())),
            permissions_map,
        }
    }
}

impl Actor for ComponentANetworkManager {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        tracing::debug!("ComponentANetworkManager started");

        // Register with router
        let manager =
            std::sync::Arc::new(self.clone()) as std::sync::Arc<dyn RoomManager + Send + Sync>;
        let register_msg = RegisterManager { manager };
        self.router.do_send(register_msg);
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        tracing::debug!("ComponentANetworkManager stopped");
    }
}

#[async_trait::async_trait]
impl RoomManager for ComponentANetworkManager {
    fn managed_rooms(&self) -> HashSet<zznet_api::types::RoomId> {
        let mut rooms = HashSet::new();
        rooms.insert(zznet_api::types::RoomId::from("room-a"));
        rooms
    }

    async fn create_for_peer(
        &self,
        peer_id: PeerId,
        role: Role,
        room_id: &RoomId,
        outbound_to_peer: tokio::sync::mpsc::Sender<(RoomId, Vec<u8>)>,
    ) -> Result<Option<RoomInboundRecipient>, CreateError> {
        tracing::debug!(
            "ComponentANetworkManager: create_for_peer called for peer {} in room {}",
            peer_id,
            room_id
        );
        if room_id.as_str() == "room-a" {
            // Translate the global Role to component-specific Permissions
            let permissions = self
                .permissions_map
                .get(role.as_str())
                .cloned()
                .ok_or_else(|| CreateError::InvalidPermission {
                    room_id: room_id.clone(),
                })?;

            // Create NetworkActor for this peer
            let network_actor =
                ComponentANetworkActor::new(peer_id.clone(), permissions, self.main_actor.clone());
            let network_actor_addr = network_actor.start();

            // Store the network actor
            if let Ok(mut actors) = self.network_actors.write() {
                actors.insert(peer_id.clone(), network_actor_addr.clone());
            }

            // Create RoomActor that handles serialization/deserialization
            let room_actor = RoomActor::new(
                room_id.clone(),
                outbound_to_peer,
                network_actor_addr.recipient::<ComponentAMessage>(),
            );
            let room_actor_addr = room_actor.start();

            // Store the room actor for outbound messaging
            if let Ok(mut actors) = self.room_actors.write() {
                actors.insert(peer_id.clone(), room_actor_addr.clone());
                tracing::debug!(
                    "ComponentANetworkManager: Stored room actor for peer {}",
                    peer_id
                );
            }

            // Return the recipient that RoomActor exposes for inbound messages
            Ok(Some(RoomActor::<ComponentAMessage>::inbound_recipient(
                &room_actor_addr,
            )))
        } else {
            Ok(None)
        }
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
        }
    }
}

impl Actor for ComponentANetworkActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        tracing::debug!(
            "ComponentANetworkActor started for peer: {:?}",
            self.peer_id
        );
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        tracing::debug!(
            "ComponentANetworkActor stopped for peer: {:?}",
            self.peer_id
        );
    }
}

/// Handle outbound messages from MainActor (broadcast to all peers)
impl Handler<ComponentAMessage> for ComponentANetworkManager {
    type Result = ();

    fn handle(&mut self, msg: ComponentAMessage, _ctx: &mut Self::Context) -> Self::Result {
        // Broadcast to all connected room actors
        if let Ok(room_actors) = self.room_actors.read() {
            tracing::debug!(
                "ComponentANetworkManager: Broadcasting message {:?} to {} peers",
                msg,
                room_actors.len()
            );
            for (peer_id, room_actor) in room_actors.iter() {
                tracing::debug!("Sending message to peer {}: {:?}", peer_id, msg);
                room_actor.do_send(msg.clone());
            }
        } else {
            tracing::error!("ComponentANetworkManager: Failed to read room_actors");
        }
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
