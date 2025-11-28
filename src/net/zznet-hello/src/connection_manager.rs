//! Manages the lifecycle of `HelloActor`s and authorizes incoming connections.
//!
//! This actor is the bridge between the transport layer and the session layer.
//! It spawns a `HelloActor` for each new connection, receives `HandshakeComplete`
//! notifications, and then wires the authenticated peer into the handover recipient.

use crate::actor::{HelloActor, HelloConfig, start_hello_actor_with_handshake_recipient};
use crate::session_messages::HandshakeComplete;
use actix::prelude::*;
use std::collections::{HashMap, HashSet};
use tokio::sync::mpsc;
use zznet_api::{AcceptTransport, OnPeerConnected, PeerId, Role, RoomId, TransportError, TransportFrame};

/// Coordinates `HelloActor`s and authorizes peers.
///
/// A `ConnectionManager` is required for any service that accepts inbound
/// connections. It ensures that every peer is authenticated and authorized
/// before being passed to the peer registry.
pub struct ConnectionManager {
    /// The target recipient for OnPeerConnected messages
    handover_recipient: Recipient<OnPeerConnected>,
    /// A map of `PeerId` to `HelloActor` address.
    hello_actors: HashMap<PeerId, Addr<HelloActor>>,
    /// The role of this service.
    pub our_role: String,
    /// The set of roles this service is allowed to connect with.
    allowed_roles: HashSet<Role>,
    /// The HelloConfig for this manager (hostname, role, rooms).
    hello_config: HelloConfig,
}

impl ConnectionManager {
    /// Creates a new `ConnectionManager`.
    pub fn new(
        handover_recipient: Recipient<OnPeerConnected>,
        hello_config: HelloConfig,
        allowed_roles: HashSet<Role>,
    ) -> Self {
        Self {
            handover_recipient,
            hello_actors: HashMap::new(),
            our_role: hello_config.our_role.clone(),
            allowed_roles,
            hello_config,
        }
    }

    /// Configures the actor to report back to this ConnectionManager upon successful handshake.
    pub(crate) fn spawn_hello_actor(
        &mut self,
        peer_id: PeerId,
        tx: mpsc::Sender<TransportFrame>,
        rx: mpsc::Receiver<Result<TransportFrame, TransportError>>,
        peer_addr: String,
        peer_identity: Option<zznet_api::PeerTLSIdentity>,
        ctx: &mut Context<Self>,
    ) -> Addr<HelloActor> {
        // Use the public API to start HelloActor with handshake recipient integration
        let addr = start_hello_actor_with_handshake_recipient(
            tx,
            rx,
            peer_addr,
            peer_identity,
            self.hello_config.clone(),
            Some(ctx.address().recipient()),
        );

        self.hello_actors.insert(peer_id, addr.clone());
        addr
    }
}

impl Actor for ConnectionManager {
    type Context = Context<Self>;
}

/// Handles a new transport connection.
impl Handler<AcceptTransport> for ConnectionManager {
    type Result = ();

    fn handle(&mut self, msg: AcceptTransport, ctx: &mut Context<Self>) {
        // Use peer_addr string as a temporary peer id until handshake provides canonical id
        let peer_id = PeerId::from(msg.peer_addr.as_str());

        // Spawn HelloActor managed by this ConnectionManager (it will wire to handshake recipient)
        let _addr = self.spawn_hello_actor(peer_id, msg.tx, msg.rx, msg.peer_addr, msg.peer_identity, ctx);
    }
}

/// Authorizes a peer after a successful handshake.
///
/// This handler is critical for security. It verifies the peer's role against
/// the `allowed_roles` set. If the role is not permitted, the connection is
/// immediately terminated.
impl Handler<HandshakeComplete> for ConnectionManager {
    type Result = ();

    fn handle(&mut self, msg: HandshakeComplete, _ctx: &mut Context<Self>) {
        let peer_id = msg.peer_id.clone();
        tracing::info!(
            "Handshake completed - peer_id: {}, peer_role_from_hello: {}, active_rooms: {:?}",
            peer_id,
            msg.peer_role_str,
            msg.active_rooms
        );

        // SECURITY: The HELLO role is the primary source of identity.
        let role = Role::new(&msg.peer_role_str);

        // Check if role is allowed
        if !self.allowed_roles.contains(&role) {
            // SECURITY: Authorization FAILED. Loud logging and immediate disconnect.
            tracing::error!(
                "!!! SECURITY REJECTION !!!: Peer {} REJECTED by authorizer",
                peer_id
            );
            tracing::error!(
                "    HELLO claimed role: {} (not in allowed roles)",
                msg.peer_role_str
            );
            tracing::warn!("Disconnecting unauthorized peer {}", peer_id);
            msg.hello_actor.do_send(crate::actor::Disconnect);
            return;
        }

        tracing::info!(
            "Peer {} authorized as {:?} (HELLO={})",
            peer_id,
            role,
            msg.peer_role_str
        );

        // Get the handover recipient and hello actor for the async task
        let handover_recipient = self.handover_recipient.clone();
        let hello_actor = msg.hello_actor.clone();

        // Spawn an async task to set up the data plane proxy
        // CRITICAL: Use actix::spawn to ensure task runs within Actix LocalSet
        actix::spawn(async move {
            let peer_id_api = zznet_api::PeerId::from(peer_id.as_str());

            // Get transport_tx from HelloActor
            let transport_tx = match hello_actor.send(crate::actor::GetTransportTx).await {
                Ok(tx) => tx,
                Err(e) => {
                    tracing::error!(
                        "Failed to get transport_tx from HelloActor for peer {}: {:?}",
                        peer_id,
                        e
                    );
                    hello_actor.do_send(crate::actor::Disconnect);
                    return;
                }
            };

            // Send OnPeerConnected to handover recipient to create rooms and get routing table
            let connect_result = handover_recipient
                .send(OnPeerConnected {
                    peer_id: peer_id_api.clone(),
                    role: role.clone(),
                    negotiated_rooms: msg
                        .active_rooms
                        .iter()
                        .map(|s| RoomId::from(s.as_str()))
                        .collect(),
                    transport_tx,
                })
                .await;

            match connect_result {
                Ok(Ok(routes)) => {
                    tracing::info!(
                        "Router created {} rooms for peer {}, configuring proxy",
                        routes.len(),
                        peer_id
                    );
                    // Send routing table to HelloActor to transition to Proxy state
                    hello_actor.do_send(crate::actor::SetRoutes(routes));
                }
                Ok(Err(error_msg)) => {
                    tracing::error!("Peer registry rejected peer {}: {}", peer_id, error_msg);
                    hello_actor.do_send(crate::actor::Disconnect);
                }
                Err(e) => {
                    tracing::error!(
                        "Failed to send to peer registry for peer {}: {:?}",
                        peer_id,
                        e
                    );
                    hello_actor.do_send(crate::actor::Disconnect);
                }
            }

            tracing::info!("Connected to peer {} as {:?}", peer_id, role);
        });
    }
}
