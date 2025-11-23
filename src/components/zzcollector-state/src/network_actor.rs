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
use tokio_stream::wrappers::{BroadcastStream, errors::BroadcastStreamRecvError};
use zznet_api::PeerId;
use zznet_room::RoomActor;

/// Message to set the room_actor address after NetworkActor creation
///
/// Used to resolve circular dependency in factory.
#[derive(Clone)]
pub(crate) struct SetRoomActor(pub Addr<RoomActor<CStateMessage>>);

impl Message for SetRoomActor {
    type Result = ();
}

/// Per-peer NetworkActor that handles protocol translation.
pub(crate) struct CStateNetworkActor {
    /// The peer ID this actor manages.
    peer_id: PeerId,

    /// Permissions for this peer (immutable, set at construction)
    permissions: crate::permissions::CStatePermissions,

    /// Link to MainActor for forwarding inbound messages.
    main_actor: Addr<crate::actor::CStateActor>,

    /// Address of the RoomActor (for sending to peer)
    /// Optional, set via SetRoomActor message after creation
    room_actor: Option<Addr<RoomActor<CStateMessage>>>,

    /// Event bus receiver for heartbeat broadcasts
    /// RAII-based subscription that auto-unsubscribes when dropped
    event_rx: tokio::sync::broadcast::Receiver<CStateEvent>,

    /// Buffer for Heartbeat messages while room_actor is not yet set
    /// Buffer used to handle startup race conditions where messages arrive before the RoomActor is wired.
    pending_heartbeats: Vec<(String, u64, u64, u64, u64, u64, u64)>,
}

impl CStateNetworkActor {
    /// Creates a new CStateNetworkActor for a specific peer.
    pub(crate) fn new(
        peer_id: PeerId,
        permissions: crate::permissions::CStatePermissions,
        main_actor: Addr<crate::actor::CStateActor>,
        event_rx: tokio::sync::broadcast::Receiver<CStateEvent>,
    ) -> Self {
        Self {
            peer_id,
            permissions,
            main_actor,
            room_actor: None,
            event_rx,
            pending_heartbeats: Vec::new(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn publish_heartbeat(
        &mut self,
        collector_id: String,
        uptime_secs: u64,
        pings_sent: u64,
        pings_received: u64,
        batches_sent: u64,
        last_config_update_ms: u64,
        connection_nonce: u64,
    ) {
        if let Some(room) = &self.room_actor {
            room.do_send(CStateMessage::Heartbeat {
                collector_id,
                uptime_secs,
                pings_sent,
                pings_received,
                batches_sent,
                last_config_update_ms,
                connection_nonce,
            });
        } else {
            self.pending_heartbeats.push((
                collector_id,
                uptime_secs,
                pings_sent,
                pings_received,
                batches_sent,
                last_config_update_ms,
                connection_nonce,
            ));
        }
    }
}

impl Actor for CStateNetworkActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        debug!("CStateNetworkActor started for peer: {:?}", self.peer_id);
        ctx.add_stream(BroadcastStream::new(self.event_rx.resubscribe()));
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

        let pending = std::mem::take(&mut self.pending_heartbeats);
        for (
            collector_id,
            uptime_secs,
            pings_sent,
            pings_received,
            batches_sent,
            last_config_update_ms,
            connection_nonce,
        ) in pending
        {
            debug!("Sending buffered heartbeat to peer {}", self.peer_id);
            self.publish_heartbeat(
                collector_id,
                uptime_secs,
                pings_sent,
                pings_received,
                batches_sent,
                last_config_update_ms,
                connection_nonce,
            );
        }
    }
}

impl StreamHandler<Result<CStateEvent, BroadcastStreamRecvError>> for CStateNetworkActor {
    fn handle(
        &mut self,
        item: Result<CStateEvent, BroadcastStreamRecvError>,
        _ctx: &mut Context<Self>,
    ) {
        match item {
            Ok(CStateEvent::HeartbeatTick {
                collector_id,
                uptime_secs,
                pings_sent,
                pings_received,
                batches_sent,
                last_config_update_ms,
                connection_nonce,
            }) => {
                debug!("Peer {:?} received heartbeat tick", self.peer_id);
                self.publish_heartbeat(
                    collector_id,
                    uptime_secs,
                    pings_sent,
                    pings_received,
                    batches_sent,
                    last_config_update_ms,
                    connection_nonce,
                );
            }
            Err(BroadcastStreamRecvError::Lagged(skipped)) => {
                debug!(
                    "Peer {:?} lagged on heartbeat events, skipped {} updates",
                    self.peer_id, skipped
                );
            }
        }
    }

    fn finished(&mut self, _ctx: &mut Context<Self>) {
        debug!(
            "Heartbeat event stream finished for peer {:?}",
            self.peer_id
        );
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
