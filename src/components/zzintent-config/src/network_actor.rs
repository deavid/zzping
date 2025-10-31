//! IntentConfig Translator Actor (Per-Peer)
//!
//! The Translator Actor in the three-actor pattern. Responsibilities:
//! - Receive typed IntentConfigNetworkMsg from RoomActor<T>
//! - Translate network messages to domain messages for MainActor (via Manager)
//! - Store peer's Role and handle authorization checks
//!
//! Lifecycle: One NetworkActor per connected peer, managed by NetworkManager

use crate::internal_messages::{InboundConfigChangeRequest, InboundGetConfigRequest};
use crate::network_messages::IntentConfigNetworkMsg;
use actix::prelude::*;
use zznet_api::types::{PeerId, Role};

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
/// - Handle authorization checks using stored Role
/// - Forward to NetworkManager for processing
/// - Receive commands from NetworkManager (SendConfigUpdateToPeer, etc.)
/// - Send typed messages to RoomActor<T>
///
/// # Message Flow
/// See `internal_messages.rs` for detailed message flow diagrams.
pub struct IntentConfigNetworkActor {
    /// ID of the peer this actor represents
    peer_id: PeerId,

    /// Role of this peer (for authorization checks)
    role: Role,

    /// Address of the NetworkManager (for forwarding inbound requests)
    manager: Addr<crate::network_manager::IntentConfigNetworkManager>,
}

impl IntentConfigNetworkActor {
    /// Create a new NetworkActor for a specific peer
    ///
    /// # Arguments
    /// - `peer_id` - ID of the peer this actor represents
    /// - `role` - Role of the peer (for authorization)
    /// - `manager` - Address of NetworkManager for forwarding requests
    pub fn new(
        peer_id: PeerId,
        role: Role,
        manager: Addr<crate::network_manager::IntentConfigNetworkManager>,
    ) -> Self {
        Self {
            peer_id,
            role,
            manager,
        }
    }

    /// Get the peer ID this actor represents
    pub fn peer_id(&self) -> &PeerId {
        &self.peer_id
    }

    /// Check if this peer is authorized for admin operations
    ///
    /// Currently checks for "client-admin" role.
    /// TODO: Make this more flexible/configurable
    fn is_authorized(&self) -> bool {
        self.role.as_str() == "client-admin"
    }
}

impl Actor for IntentConfigNetworkActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        log::debug!(
            "IntentConfigNetworkActor started for peer: {}",
            self.peer_id
        );
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        log::debug!(
            "IntentConfigNetworkActor stopped for peer: {}",
            self.peer_id
        );
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

                // Check authorization using stored role
                if !self.is_authorized() {
                    log::warn!(
                        "Peer {} with role '{}' is not authorized for config changes (requires 'client-admin')",
                        self.peer_id,
                        self.role.as_str()
                    );

                    // Send error back via Manager
                    self.manager
                        .do_send(crate::internal_messages::SendErrorToPeer {
                            peer_id: self.peer_id.clone(),
                            error_message: "Unauthorized: ClientAdmin role required".to_string(),
                        });
                    return;
                }

                log::debug!(
                    "Peer {} AUTHORIZED for config change (role: {})",
                    self.peer_id,
                    self.role.as_str()
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

#[cfg(test)]
mod tests {
    // Phase 3.9 COMPLETE: Integration tests in tests/three_actor_integration_tests.rs
    // These tests cover the happy-path scenarios for the three-actor pattern.
    // Unit tests for individual NetworkActor methods are not required at this stage.
}
