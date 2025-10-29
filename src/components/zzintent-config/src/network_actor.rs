//! IntentConfig Network Actor (Per-Peer)
//!
//! The Network Actor in the three-actor pattern. Responsibilities:
//! - Hold Room<IntentConfigNetworkMsg> for one specific peer
//! - Receive messages from Room (inbound from peer)
//! - Deserialize and validate messages
//! - Translate network messages to domain messages for MainActor (via Manager)
//! - Send responses back through Room (outbound to peer)
//!
//! Lifecycle: One NetworkActor per connected peer, managed by NetworkManager

use crate::internal_messages::{
    InboundConfigChangeRequest, InboundGetConfigRequest, SendConfigUpdateToPeer,
    SendErrorMessageToPeer,
};
use crate::network_messages::IntentConfigNetworkMsg;
use actix::prelude::*;
use tokio::sync::{broadcast, mpsc};
use zznet_api::types::{PeerId, RoomId};

/// IntentConfigNetworkActor - Handles protocol for one peer
///
/// This actor exists for the lifetime of a peer connection and handles
/// all network protocol translation for that specific peer. It is the
/// boundary between the network (Room<T>) and the business logic (MainActor).
///
/// # Lifecycle
/// - Created by NetworkManager when peer joins "intent-config" room
/// - Destroyed by NetworkManager when peer disconnects
/// - One actor per peer (NOT shared)
///
/// # Responsibilities
/// - Receive IntentConfigNetworkMsg from Room<T>
/// - Deserialize and validate messages
/// - Translate to domain messages (InboundConfigChangeRequest, etc.)
/// - Forward to NetworkManager for processing
/// - Receive commands from NetworkManager (SendConfigUpdateToPeer, etc.)
/// - Serialize and send via Room<T>
///
/// # Message Flow
/// See `internal_messages.rs` for detailed message flow diagrams.
pub struct IntentConfigNetworkActor {
    /// ID of the peer this actor represents
    peer_id: PeerId,

    /// Address of the NetworkManager (for forwarding inbound requests)
    manager: Addr<crate::network_manager::IntentConfigNetworkManager>,

    /// Channel for sending messages to the peer (Phase 3.5)
    /// Obtained from SessionManager via GetPeerSender
    peer_sender: Option<mpsc::Sender<(RoomId, Vec<u8>)>>,

    /// Receiver for messages from the peer (Phase 3.5)
    /// Obtained from SessionManager via SubscribePeerInbound
    peer_receiver: Option<broadcast::Receiver<(RoomId, Vec<u8>)>>,
}

impl IntentConfigNetworkActor {
    /// Create a new NetworkActor for a specific peer
    ///
    /// # Arguments
    /// - `peer_id` - ID of the peer this actor represents
    /// - `manager` - Address of NetworkManager for forwarding requests
    /// - `peer_sender` - Channel for sending messages to peer (from SessionManager)
    /// - `peer_receiver` - Channel for receiving messages from peer (from SessionManager)
    pub fn new(
        peer_id: PeerId,
        manager: Addr<crate::network_manager::IntentConfigNetworkManager>,
        peer_sender: mpsc::Sender<(RoomId, Vec<u8>)>,
        peer_receiver: broadcast::Receiver<(RoomId, Vec<u8>)>,
    ) -> Self {
        Self {
            peer_id,
            manager,
            peer_sender: Some(peer_sender),
            peer_receiver: Some(peer_receiver),
        }
    }

    /// Get the peer ID this actor represents
    pub fn peer_id(&self) -> &PeerId {
        &self.peer_id
    }

    /// Send a message to the peer via SessionManager channels
    ///
    /// Phase 3.5: Now implemented with real SessionManager channels
    async fn send_to_peer(&self, msg: IntentConfigNetworkMsg) -> Result<(), String> {
        let peer_sender = self
            .peer_sender
            .as_ref()
            .ok_or("Peer sender channel not available")?;

        // Serialize the message
        let bytes = bincode::serde::encode_to_vec(&msg, bincode::config::standard())
            .map_err(|e| format!("Serialization failed: {}", e))?;

        // Send to peer via "intent-config" room
        let room_id = RoomId::from("intent-config");
        peer_sender
            .send((room_id, bytes))
            .await
            .map_err(|_| "Failed to send message to peer (channel closed)".to_string())?;

        log::debug!("Sent message to peer {}: {:?}", self.peer_id, msg);
        Ok(())
    }
}

impl Actor for IntentConfigNetworkActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        log::debug!(
            "IntentConfigNetworkActor started for peer: {}",
            self.peer_id
        );

        // Phase 3.5: Spawn task to listen for inbound messages from peer
        if let Some(mut receiver) = self.peer_receiver.take() {
            let peer_id = self.peer_id.clone();
            let addr = ctx.address();

            let fut = async move {
                loop {
                    match receiver.recv().await {
                        Ok((room_id, bytes)) => {
                            // Only process messages for "intent-config" room
                            if room_id.as_str() != "intent-config" {
                                log::warn!(
                                    "Received message for wrong room {:?}, expected intent-config",
                                    room_id
                                );
                                continue;
                            }

                            // Deserialize the message
                            match bincode::serde::decode_from_slice::<IntentConfigNetworkMsg, _>(
                                &bytes,
                                bincode::config::standard(),
                            ) {
                                Ok((msg, _)) => {
                                    log::debug!(
                                        "Received message from peer {}: {:?}",
                                        peer_id,
                                        msg
                                    );
                                    // Forward to self for processing
                                    addr.do_send(msg);
                                }
                                Err(e) => {
                                    log::error!(
                                        "Failed to deserialize message from peer {}: {}",
                                        peer_id,
                                        e
                                    );
                                }
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            log::info!("Peer {} channel closed", peer_id);
                            break;
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            log::warn!("Peer {} channel lagged by {} messages", peer_id, n);
                            // Continue receiving - lagged messages are skipped
                        }
                    }
                }
                log::debug!("Inbound message task stopped for peer {}", peer_id);
            };

            ctx.spawn(fut.into_actor(self));
        } else {
            log::warn!(
                "No peer receiver available for peer {} - cannot receive messages",
                self.peer_id
            );
        }
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        log::debug!(
            "IntentConfigNetworkActor stopped for peer: {}",
            self.peer_id
        );

        // Room<T> cleanup will be added when Room<T> integration is complete
    }
}

// ============================================================================
// Handler: IntentConfigNetworkMsg (from Room<T>)
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

                // Translate to domain message and forward to Manager
                self.manager.do_send(InboundConfigChangeRequest {
                    peer_id: PeerId::from(sender_peer_id.as_str()),
                    targets,
                    ping_rate_pps,
                });
            }

            IntentConfigNetworkMsg::QueryCurrentConfig => {
                log::debug!("Peer {} requesting current config", self.peer_id);

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

            IntentConfigNetworkMsg::ConfigUpdate { .. } => {
                log::warn!(
                    "Received ConfigUpdate from peer {} - ignoring (only Database sends these)",
                    self.peer_id
                );
            }

            IntentConfigNetworkMsg::CurrentConfig { .. } => {
                log::warn!(
                    "Received CurrentConfig from peer {} - ignoring (only Database sends these)",
                    self.peer_id
                );
            }

            IntentConfigNetworkMsg::Heartbeat => {
                log::trace!("Received Heartbeat from peer {}", self.peer_id);
                // Heartbeat timestamping deferred - not critical for Phase 3
            }

            IntentConfigNetworkMsg::Error { reason } => {
                log::error!("Received error from peer {}: {}", self.peer_id, reason);
                // Error forwarding deferred - errors are already logged
            }
        }
    }
}

// ============================================================================
// Handler: SendConfigUpdateToPeer (from NetworkManager)
// ============================================================================

impl Handler<SendConfigUpdateToPeer> for IntentConfigNetworkActor {
    type Result = ResponseActFuture<Self, ()>;

    fn handle(&mut self, msg: SendConfigUpdateToPeer, _ctx: &mut Self::Context) -> Self::Result {
        log::debug!("Sending config update to peer: {}", self.peer_id);

        let network_msg = IntentConfigNetworkMsg::ConfigUpdate {
            targets: msg.config.targets,
            ping_rate_pps: msg.config.ping_rate_pps,
        };

        let peer_sender = self.peer_sender.clone();
        let peer_id = self.peer_id.clone();

        let fut = async move {
            if let Some(sender) = peer_sender {
                // Serialize the message
                match bincode::serde::encode_to_vec(&network_msg, bincode::config::standard()) {
                    Ok(bytes) => {
                        let room_id = RoomId::from("intent-config");
                        if let Err(e) = sender.send((room_id, bytes)).await {
                            log::error!("Failed to send config update to peer {}: {}", peer_id, e);
                        } else {
                            log::debug!("Config update sent to peer {}", peer_id);
                        }
                    }
                    Err(e) => {
                        log::error!(
                            "Failed to serialize config update for peer {}: {}",
                            peer_id,
                            e
                        );
                    }
                }
            } else {
                log::error!("No peer sender available for peer {}", peer_id);
            }
        }
        .into_actor(self);

        Box::pin(fut)
    }
}

// ============================================================================
// Handler: SendErrorMessageToPeer (from NetworkManager)
// ============================================================================

impl Handler<SendErrorMessageToPeer> for IntentConfigNetworkActor {
    type Result = ResponseActFuture<Self, ()>;

    fn handle(&mut self, msg: SendErrorMessageToPeer, _ctx: &mut Self::Context) -> Self::Result {
        log::debug!(
            "Sending error message to peer {}: {}",
            self.peer_id,
            msg.error_message
        );

        let network_msg = IntentConfigNetworkMsg::Error {
            reason: msg.error_message,
        };

        let peer_sender = self.peer_sender.clone();
        let peer_id = self.peer_id.clone();

        let fut = async move {
            if let Some(sender) = peer_sender {
                // Serialize the message
                match bincode::serde::encode_to_vec(&network_msg, bincode::config::standard()) {
                    Ok(bytes) => {
                        let room_id = RoomId::from("intent-config");
                        if let Err(e) = sender.send((room_id, bytes)).await {
                            log::error!("Failed to send error message to peer {}: {}", peer_id, e);
                        } else {
                            log::debug!("Error message sent to peer {}", peer_id);
                        }
                    }
                    Err(e) => {
                        log::error!(
                            "Failed to serialize error message for peer {}: {}",
                            peer_id,
                            e
                        );
                    }
                }
            } else {
                log::error!("No peer sender available for peer {}", peer_id);
            }
        }
        .into_actor(self);

        Box::pin(fut)
    }
}

#[cfg(test)]
mod tests {
    // Phase 3.9 COMPLETE: Integration tests in tests/three_actor_integration_tests.rs
    // These tests cover the happy-path scenarios for the three-actor pattern.
    // Unit tests for individual NetworkActor methods are not required at this stage.
}
