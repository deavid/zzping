//! Per-peer Translator Actor for the CState component.
//!
//! Each NetworkActor handles protocol translation for a single peer connection.
//! It translates between network messages (CStateMessage) and internal messages.

use crate::{
    events::CStateEvent,
    internal_messages::{
        InboundCollectorList, InboundHeartbeat, InboundHeartbeatAck, InboundQueryCollectors,
        InboundRegistrationRejected, InboundUnauthorized,
    },
    network_messages::CStateMessage,
};
use actix::prelude::*;
use log::debug;
use tokio::sync::broadcast::error::RecvError;
use zznet_api::types::PeerId;
use zznet_room::actor::RoomActor;

/// Message to set the room_actor address after NetworkActor creation
///
/// Used to resolve circular dependency in factory.
#[derive(Clone)]
pub struct SetRoomActor(pub Addr<RoomActor<CStateMessage>>);

impl Message for SetRoomActor {
    type Result = ();
}

/// Per-peer NetworkActor that handles protocol translation.
pub struct CStateNetworkActor {
    /// The peer ID this actor manages.
    peer_id: PeerId,

    /// Permissions for this peer (immutable, set at construction)
    permissions: crate::permissions::CStatePermissions,

    /// Link to MainActor for forwarding inbound messages.
    main_actor: Addr<crate::actor::CStateActor>,

    /// Link to NetworkManager for error reporting (reserved for future use).
    #[allow(dead_code)]
    manager: Addr<crate::network_manager::CStateNetworkManager>,

    /// Address of the RoomActor (for sending to peer)
    /// Optional, set via SetRoomActor message after creation
    room_actor: Option<Addr<RoomActor<CStateMessage>>>,

    /// Event bus receiver for heartbeat broadcasts
    /// RAII-based subscription that auto-unsubscribes when dropped
    event_rx: tokio::sync::broadcast::Receiver<CStateEvent>,

    /// Buffer for Heartbeat messages while room_actor is not yet set
    /// Phase 9: Used to handle startup race condition where messages arrive before SetRoomActor
    pending_heartbeats: Vec<(String, u64, u64, u64, u64, u64, u64)>,
}

impl CStateNetworkActor {
    /// Creates a new CStateNetworkActor for a specific peer.
    pub fn new(
        peer_id: PeerId,
        permissions: crate::permissions::CStatePermissions,
        main_actor: Addr<crate::actor::CStateActor>,
        manager: Addr<crate::network_manager::CStateNetworkManager>,
        event_rx: tokio::sync::broadcast::Receiver<CStateEvent>,
    ) -> Self {
        Self {
            peer_id,
            permissions,
            main_actor,
            manager,
            room_actor: None,
            event_rx,
            pending_heartbeats: Vec::new(),
        }
    }
}

impl Actor for CStateNetworkActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        debug!("CStateNetworkActor started for peer: {:?}", self.peer_id);

        // Spawn event listener task for heartbeat broadcasts
        let mut rx = self.event_rx.resubscribe();
        let peer_id = self.peer_id.clone();
        let addr = ctx.address();

        ctx.spawn(
            async move {
                loop {
                    match rx.recv().await {
                        Ok(event) => {
                            match event {
                                CStateEvent::HeartbeatTick {
                                    collector_id,
                                    uptime_secs,
                                    pings_sent,
                                    pings_received,
                                    batches_sent,
                                    last_config_update_ms,
                                    connection_nonce,
                                } => {
                                    debug!("Peer {} received heartbeat event", peer_id);
                                    // Send to self to handle with room_actor check
                                    addr.do_send(CStateMessage::Heartbeat {
                                        collector_id,
                                        uptime_secs,
                                        pings_sent,
                                        pings_received,
                                        batches_sent,
                                        last_config_update_ms,
                                        connection_nonce,
                                    });
                                }
                            }
                        }
                        Err(RecvError::Lagged(skipped)) => {
                            // Log lag but continue receiving updates
                            debug!(
                                "Peer {} lagged on heartbeat events, skipped {} updates",
                                peer_id, skipped
                            );
                        }
                        Err(RecvError::Closed) => {
                            // Broadcast sender dropped, unsubscribe
                            debug!("Event listener stopped for peer {}", peer_id);
                            break;
                        }
                    }
                }
            }
            .into_actor(self),
        );
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        debug!("CStateNetworkActor stopped for peer: {:?}", self.peer_id);
        // event_rx drops here → automatic unsubscribe (RAII)
    }
}

// ============================================================================
// SetRoomActor Handler
// ============================================================================

impl Handler<SetRoomActor> for CStateNetworkActor {
    type Result = ();

    fn handle(&mut self, msg: SetRoomActor, _ctx: &mut Context<Self>) -> Self::Result {
        debug!("Setting room_actor for peer {}", self.peer_id);
        self.room_actor = Some(msg.0.clone());

        // Drain any pending messages that arrived before room_actor was set
        for (
            collector_id,
            uptime_secs,
            pings_sent,
            pings_received,
            batches_sent,
            last_config_update_ms,
            connection_nonce,
        ) in self.pending_heartbeats.drain(..)
        {
            if let Some(ref room) = self.room_actor {
                debug!("Sending buffered heartbeat to peer {}", self.peer_id);
                room.do_send(CStateMessage::Heartbeat {
                    collector_id,
                    uptime_secs,
                    pings_sent,
                    pings_received,
                    batches_sent,
                    last_config_update_ms,
                    connection_nonce,
                });
            }
        }
    }
}

// ============================================================================
// Inbound Message Handlers (RoomActor<T> → NetworkActor → MainActor)
// ============================================================================

impl Handler<CStateMessage> for CStateNetworkActor {
    type Result = ResponseFuture<()>;

    fn handle(&mut self, msg: CStateMessage, _ctx: &mut Context<Self>) -> Self::Result {
        debug!(
            "Received network message from peer {:?}: {:?}",
            self.peer_id, msg
        );

        let main_actor = self.main_actor.clone();
        let peer_id = self.peer_id.clone();
        let room_actor = self.room_actor.clone();

        match msg {
            CStateMessage::Heartbeat {
                collector_id,
                uptime_secs,
                pings_sent,
                pings_received,
                batches_sent,
                last_config_update_ms,
                connection_nonce,
            } => {
                // Phase 9: Buffer if room_actor not yet set
                if self.room_actor.is_none() {
                    debug!(
                        "Buffering heartbeat for peer {} (room_actor not yet set)",
                        self.peer_id
                    );
                    self.pending_heartbeats.push((
                        collector_id,
                        uptime_secs,
                        pings_sent,
                        pings_received,
                        batches_sent,
                        last_config_update_ms,
                        connection_nonce,
                    ));
                    return Box::pin(async move {
                        // Return immediately, buffered message will be sent when room_actor is set
                    });
                }

                // Enforce permission: only peers with can_send_heartbeat can send heartbeats
                if !self.permissions.can_send_heartbeat {
                    debug!(
                        "Peer {:?} attempted to send heartbeat without permission",
                        peer_id
                    );
                    let fut = async move {
                        if let Some(room) = room_actor {
                            room.do_send(CStateMessage::Unauthorized {
                                reason: "Not authorized to send heartbeats".to_string(),
                            });
                        }
                    };
                    return Box::pin(fut);
                }

                let fut = async move {
                    let request = InboundHeartbeat {
                        peer_id: peer_id.clone(),
                        collector_id,
                        uptime_secs,
                        pings_sent,
                        pings_received,
                        batches_sent,
                        last_config_update_ms,
                        connection_nonce,
                    };

                    match main_actor.send(request).await {
                        Ok(Ok(ack_response)) => {
                            debug!("Heartbeat accepted for peer {}", peer_id);
                            if let Some(room) = room_actor {
                                if let Some(reason) = ack_response.rejection {
                                    room.do_send(CStateMessage::RegistrationRejected { reason });
                                } else {
                                    room.do_send(CStateMessage::HeartbeatAck {
                                        timestamp_ms: ack_response.timestamp_ms,
                                        server_time_ms: ack_response.server_time_ms,
                                    });
                                }
                            }
                        }
                        Ok(Err(e)) => {
                            debug!("Heartbeat rejected: {}", e);
                            if let Some(room) = room_actor {
                                room.do_send(CStateMessage::Unauthorized { reason: e });
                            }
                        }
                        Err(e) => {
                            debug!("MainActor error: {}", e);
                        }
                    }
                };
                Box::pin(fut)
            }

            CStateMessage::HeartbeatAck {
                timestamp_ms,
                server_time_ms,
            } => {
                // HeartbeatAck is a response message, fire-and-forget
                main_actor.do_send(InboundHeartbeatAck {
                    peer_id,
                    timestamp_ms,
                    server_time_ms,
                });
                Box::pin(async {})
            }

            CStateMessage::QueryCollectors => {
                // Enforce permission: only peers with can_query_collectors can query
                if !self.permissions.can_query_collectors {
                    debug!(
                        "Peer {:?} attempted to query collectors without permission",
                        peer_id
                    );
                    let fut = async move {
                        if let Some(room) = room_actor {
                            room.do_send(CStateMessage::Unauthorized {
                                reason: "Not authorized to query collectors".to_string(),
                            });
                        }
                    };
                    return Box::pin(fut);
                }

                let fut = async move {
                    let request = InboundQueryCollectors {
                        peer_id: peer_id.clone(),
                    };

                    match main_actor.send(request).await {
                        Ok(Ok(collectors)) => {
                            debug!("Query returned {} collectors", collectors.len());
                            if let Some(room) = room_actor {
                                room.do_send(CStateMessage::CollectorList { collectors });
                            }
                        }
                        Ok(Err(e)) => {
                            debug!("Query rejected: {}", e);
                            if let Some(room) = room_actor {
                                room.do_send(CStateMessage::Unauthorized { reason: e });
                            }
                        }
                        Err(e) => {
                            debug!("MainActor error: {}", e);
                        }
                    }
                };
                Box::pin(fut)
            }

            CStateMessage::CollectorList { collectors } => {
                main_actor.do_send(InboundCollectorList {
                    peer_id,
                    collectors,
                });
                Box::pin(async {})
            }

            CStateMessage::RegistrationRejected { reason } => {
                main_actor.do_send(InboundRegistrationRejected { peer_id, reason });
                Box::pin(async {})
            }

            CStateMessage::Unauthorized { reason } => {
                main_actor.do_send(InboundUnauthorized { peer_id, reason });
                Box::pin(async {})
            }
        }
    }
}
