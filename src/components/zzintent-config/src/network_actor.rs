//! IntentConfig Translator Actor (Per-Peer)
//!
//! The Translator Actor in the three-actor pattern. Responsibilities:
//! - Receive typed IntentConfigNetworkMsg from RoomActor<T>
//! - Translate network messages to domain messages for MainActor (via Manager)
//! - Store peer's Role and handle authorization checks
//!
//! Lifecycle: One NetworkActor per connected peer, managed by NetworkManager

use crate::events::IntentConfigEvent;
use crate::internal_messages::{InboundConfigChangeRequest, InboundGetConfigRequest};
use crate::network_messages::IntentConfigNetworkMsg;
use crate::permissions::IntentConfigPermissions;
use actix::prelude::*;
use tokio::sync::broadcast::error::RecvError;
use zznet_api::PeerId;
use zznet_room::RoomActor;

/// Message to set the room_actor address after NetworkActor creation
///
/// Used to resolve circular dependency in factory.
/// Factory creates NetworkActor first, then RoomActor, then wires them together.
#[derive(Clone)]
pub(crate) struct SetRoomActor(pub Addr<RoomActor<IntentConfigNetworkMsg>>);

impl Message for SetRoomActor {
    type Result = ();
}

/// IntentConfigNetworkActor - Handles protocol translation for one peer
///
/// This actor exists for the lifetime of a peer connection and handles
/// all message translation for that specific peer. It is the
/// boundary between the network (RoomActor<T>) and the business logic (MainActor).
///
/// # Lifecycle
/// - Created by NetworkManager when peer joins "intent-config" room
/// - Destroyed by NetworkManager when peer disconnects
/// - One actor per peer (NOT shared)
///
/// # Responsibilities
/// - Receive IntentConfigNetworkMsg from RoomActor<T>
/// - Translate to domain messages (InboundConfigChangeRequest, etc.)
/// - Handle authorization checks using stored Permissions
/// - Forward to MainActor with request/reply pattern
/// - Subscribe to config change events from MainActor
/// - Send typed messages to RoomActor<T>
///
/// # Message Flow
/// See `internal_messages.rs` for detailed message flow diagrams.
pub(crate) struct IntentConfigNetworkActor {
    /// ID of the peer this actor represents
    peer_id: PeerId,

    /// Permissions of this peer (for authorization checks)
    permissions: IntentConfigPermissions,

    /// Address of the NetworkManager (for inbound request forwarding)
    manager: Addr<crate::network_manager::IntentConfigNetworkManager>,

    /// Address of the RoomActor (for sending to peer)
    /// Optional, set via SetRoomActor message after creation
    room_actor: Option<Addr<RoomActor<IntentConfigNetworkMsg>>>,

    /// Event bus receiver for config changes
    /// RAII-based subscription that auto-unsubscribes when dropped
    event_rx: tokio::sync::broadcast::Receiver<IntentConfigEvent>,

    /// Buffer for ConfigUpdate messages while room_actor is not yet set
    /// Used to handle startup race condition where messages arrive before SetRoomActor
    pending_updates: Vec<(Vec<std::net::IpAddr>, u64)>,
}

impl IntentConfigNetworkActor {
    /// Create a new NetworkActor for a specific peer
    ///
    /// Note: room_actor must be set via SetRoomActor message after creation
    pub(crate) fn new(
        peer_id: PeerId,
        permissions: IntentConfigPermissions,
        manager: Addr<crate::network_manager::IntentConfigNetworkManager>,
        event_rx: tokio::sync::broadcast::Receiver<IntentConfigEvent>,
    ) -> Self {
        Self {
            peer_id,
            permissions,
            manager,
            room_actor: None,
            event_rx,
            pending_updates: Vec::new(),
        }
    }
}

impl Actor for IntentConfigNetworkActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        log::debug!(
            "IntentConfigNetworkActor started for peer: {}",
            self.peer_id
        );

        // Spawn event listener task
        // Each NetworkActor subscribes to config change events
        let mut rx = self.event_rx.resubscribe();
        let peer_id = self.peer_id.clone();
        let addr = ctx.address();

        ctx.spawn(
            async move {
                loop {
                    match rx.recv().await {
                        Ok(event) => {
                            match event {
                                IntentConfigEvent::ConfigChanged(config) => {
                                    log::debug!("Peer {} received config change event", peer_id);
                                    // Send to self to handle with room_actor check
                                    addr.do_send(IntentConfigNetworkMsg::ConfigUpdate {
                                        targets: config.targets,
                                        ping_rate_pps: config.ping_rate_pps,
                                    });
                                }
                            }
                        }
                        Err(RecvError::Lagged(skipped)) => {
                            // Log lag but continue receiving updates
                            log::warn!(
                                "Peer {} lagged on config events, skipped {} updates",
                                peer_id,
                                skipped
                            );
                        }
                        Err(RecvError::Closed) => {
                            // Broadcast sender dropped, unsubscribe
                            log::debug!("Event listener stopped for peer {}", peer_id);
                            break;
                        }
                    }
                }
            }
            .into_actor(self),
        );
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        log::debug!(
            "IntentConfigNetworkActor stopped for peer: {}",
            self.peer_id
        );
        // event_rx drops here → automatic unsubscribe (RAII)
    }
}

// ============================================================================
// Handler: SetRoomActor (from Factory)
// ============================================================================

impl Handler<SetRoomActor> for IntentConfigNetworkActor {
    type Result = ();

    fn handle(&mut self, msg: SetRoomActor, _ctx: &mut Self::Context) -> Self::Result {
        log::debug!("Setting room_actor for peer: {}", self.peer_id);
        self.room_actor = Some(msg.0.clone());

        // Drain any pending messages that arrived before room_actor was set
        for (targets, ping_rate_pps) in self.pending_updates.drain(..) {
            if let Some(ref room) = self.room_actor {
                log::debug!("Sending buffered config update to peer {}", self.peer_id);
                room.do_send(IntentConfigNetworkMsg::ConfigUpdate {
                    targets,
                    ping_rate_pps,
                });
            }
        }
    }
}

// ============================================================================
// Handler: IntentConfigNetworkMsg (from RoomActor<T>)
// ============================================================================

impl Handler<IntentConfigNetworkMsg> for IntentConfigNetworkActor {
    type Result = ();

    fn handle(&mut self, msg: IntentConfigNetworkMsg, _ctx: &mut Self::Context) -> Self::Result {
        log::debug!(
            "Received network message from peer {}: {:?}",
            self.peer_id,
            msg
        );

        match msg {
            // ConfigUpdate from event listener - forward to room_actor
            IntentConfigNetworkMsg::ConfigUpdate {
                targets,
                ping_rate_pps,
            } => {
                if let Some(ref room) = self.room_actor {
                    log::debug!("Sending config update to peer {}", self.peer_id);
                    room.do_send(IntentConfigNetworkMsg::ConfigUpdate {
                        targets,
                        ping_rate_pps,
                    });
                } else {
                    log::warn!(
                        "Cannot send config update to peer {} - room_actor not yet set",
                        self.peer_id
                    );
                }
            }

            IntentConfigNetworkMsg::RequestConfigChange {
                sender_peer_id,
                targets,
                ping_rate_pps,
            } => {
                log::info!(
                    "Peer {} requesting config change (sender: {}): targets={:?}, rate={}",
                    self.peer_id,
                    sender_peer_id,
                    targets,
                    ping_rate_pps
                );

                // Check authorization using stored permissions
                if !self.permissions.can_write_config {
                    log::warn!(
                        "Peer {} is not authorized for config changes (requires can_write_config permission)",
                        self.peer_id
                    );
                    return;
                }

                log::debug!(
                    "Peer {} AUTHORIZED for config change (has write permission)",
                    self.peer_id
                );

                // Translate to domain message and forward to Manager
                self.manager.do_send(InboundConfigChangeRequest {
                    peer_id: PeerId::from(sender_peer_id.as_str()),
                    targets,
                    ping_rate_pps,
                });
            }

            IntentConfigNetworkMsg::QueryCurrentConfig => {
                log::debug!("Peer {} requesting current config", self.peer_id);

                // Check authorization using stored permissions
                if !self.permissions.can_read_config {
                    log::warn!(
                        "Peer {} is not authorized to read config (requires can_read_config permission)",
                        self.peer_id
                    );
                    return;
                }

                // Forward to Manager for processing
                let manager = self.manager.clone();
                let peer_id = self.peer_id.clone();

                let fut = async move {
                    let config = manager
                        .send(InboundGetConfigRequest {
                            peer_id: peer_id.clone(),
                        })
                        .await;

                    match config {
                        Ok(_config) => {
                            log::debug!("Got config from Manager for peer: {}", peer_id);
                            // CurrentConfig response deferred until Room<T> send API is available
                            log::warn!(
                                "CurrentConfig send not implemented - Room<T> integration pending"
                            );
                        }
                        Err(e) => {
                            log::error!(
                                "Failed to get config from Manager for peer {}: {}",
                                peer_id,
                                e
                            );
                        }
                    }
                };

                // Spawn as background task
                actix::spawn(fut);
            }

            IntentConfigNetworkMsg::CurrentConfig { .. } => {
                log::warn!(
                    "Received CurrentConfig from peer {} - ignoring (only Database sends these)",
                    self.peer_id
                );
            }

            IntentConfigNetworkMsg::Heartbeat => {
                log::trace!("Received Heartbeat from peer {}", self.peer_id);
            }

            IntentConfigNetworkMsg::Error { reason } => {
                log::error!("Received error from peer {}: {}", self.peer_id, reason);
                // Error forwarding deferred - errors are already logged
            }
        }
    }
}
