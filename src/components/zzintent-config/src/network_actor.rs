//! IntentConfig Translator Actor (Per-Peer)
//!
//! The Translator Actor in the three-actor pattern. Responsibilities:
//! - Receive typed IntentConfigNetworkMsg from RoomActor<T>
//! - Translate network messages to domain messages for MainActor
//! - Store peer's Role and handle authorization checks
//!
//! Lifecycle: One NetworkActor per connected peer, managed by NetworkManager

use crate::actor::IntentConfigActor;
use crate::events::IntentConfigEvent;
use crate::internal_messages::{InboundConfigChangeRequest, InboundGetConfigRequest};
use crate::network_messages::IntentConfigNetworkMsg;
use crate::permissions::IntentConfigPermissions;
use actix::prelude::*;
use std::net::IpAddr;
use tokio_stream::wrappers::{BroadcastStream, errors::BroadcastStreamRecvError};
use zznet_api::PeerId;
use zznet_room::RoomActor;

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

    /// Address of the MainActor for directly forwarding inbound messages
    main_actor: Addr<IntentConfigActor>,

    /// Address of the RoomActor (for sending to peer)
    /// Optional, set via SetRoomActor message after creation
    room_actor: Option<Addr<RoomActor<IntentConfigNetworkMsg>>>,

    /// Event bus receiver for config changes
    /// RAII-based subscription that auto-unsubscribes when dropped
    event_rx: tokio::sync::broadcast::Receiver<IntentConfigEvent>,

    /// Buffer for ConfigUpdate messages while room_actor is not yet set
    /// Used to handle startup race condition where messages arrive before SetRoomActor
    pending_updates: Vec<(Vec<IpAddr>, u64)>,
}

impl IntentConfigNetworkActor {
    /// Create a new NetworkActor for a specific peer
    ///
    /// Note: room_actor must be set via SetRoomActor message after creation
    pub(crate) fn new(
        peer_id: PeerId,
        permissions: IntentConfigPermissions,
        main_actor: Addr<IntentConfigActor>,
        event_rx: tokio::sync::broadcast::Receiver<IntentConfigEvent>,
    ) -> Self {
        Self {
            peer_id,
            permissions,
            main_actor,
            room_actor: None,
            event_rx,
            pending_updates: Vec::new(),
        }
    }

    fn publish_config_update(&mut self, targets: Vec<IpAddr>, ping_rate_pps: u64) {
        if !self.permissions.can_read_config {
            log::debug!(
                "Peer {} not authorized to read config, skipping broadcast",
                self.peer_id
            );
            return;
        }

        if let Some(room) = &self.room_actor {
            room.do_send(IntentConfigNetworkMsg::ConfigUpdate {
                targets,
                ping_rate_pps,
            });
        } else {
            self.pending_updates.push((targets, ping_rate_pps));
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

        ctx.add_stream(BroadcastStream::new(self.event_rx.resubscribe()));
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

impl Handler<zznet_component::SetRoomActor<IntentConfigNetworkMsg>> for IntentConfigNetworkActor {
    type Result = ();

    fn handle(&mut self, msg: zznet_component::SetRoomActor<IntentConfigNetworkMsg>, _ctx: &mut Self::Context) -> Self::Result {
        log::debug!("Setting room_actor for peer: {}", self.peer_id);
        self.room_actor = Some(msg.0.clone());

        let pending = std::mem::take(&mut self.pending_updates);
        for (targets, ping_rate_pps) in pending {
            log::debug!("Sending buffered config update to peer {}", self.peer_id);
            self.publish_config_update(targets, ping_rate_pps);
        }
    }
}

impl StreamHandler<Result<IntentConfigEvent, BroadcastStreamRecvError>>
    for IntentConfigNetworkActor
{
    fn handle(
        &mut self,
        item: Result<IntentConfigEvent, BroadcastStreamRecvError>,
        _ctx: &mut Context<Self>,
    ) {
        match item {
            Ok(IntentConfigEvent::ConfigChanged(config)) => {
                log::debug!("Peer {} received config change event", self.peer_id);
                self.publish_config_update(config.targets, config.ping_rate_pps);
            }
            Err(BroadcastStreamRecvError::Lagged(skipped)) => {
                log::warn!(
                    "Peer {} lagged on config events, skipped {} updates",
                    self.peer_id,
                    skipped
                );
            }
        }
    }

    fn finished(&mut self, _ctx: &mut Context<Self>) {
        log::debug!("Event stream finished for peer {}", self.peer_id);
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
                self.publish_config_update(targets, ping_rate_pps);
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

                // Translate to domain message and forward directly to MainActor
                self.main_actor.do_send(InboundConfigChangeRequest {
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

                // Forward directly to MainActor for processing
                let main_actor = self.main_actor.clone();
                let peer_id = self.peer_id.clone();
                let room = self.room_actor.clone();

                let fut = async move {
                    let config = main_actor
                        .send(InboundGetConfigRequest {
                            peer_id: peer_id.clone(),
                        })
                        .await;

                    match config {
                        Ok(current) => {
                            log::debug!("Got config from MainActor for peer: {}", peer_id);
                            if let Some(room) = room {
                                room.do_send(IntentConfigNetworkMsg::CurrentConfig {
                                    targets: current.targets,
                                    ping_rate_pps: current.ping_rate_pps,
                                });
                            }
                        }
                        Err(e) => {
                            log::error!(
                                "Failed to get config from MainActor for peer {}: {}",
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
