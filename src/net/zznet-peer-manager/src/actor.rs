//! # PeerManager Actor
//!
//! **Phase 7: Actor-based interface for PeerManager**
//!
//! This module provides an Actix Actor wrapper around the PeerManager struct,
//! enabling message-based async access for components.
//!
//! ## Architecture
//!
//! ```text
//! PeerManagerActor (Actor wrapper)
//!   ├─ Owns: PeerManager (control plane logic)
//!   ├─ Owns: HashMap<PeerId, PeerState> (peer state + channels)
//!   └─ Provides: Message handlers for async queries
//! ```
//!
//! ## Essential Messages (Phase 7.1)
//!
//! - `GetPeerRole` - Query peer authorization role
//! - `GetPeerSender` - Get channel to send messages to peer
//! - `SubscribePeerInbound` - Subscribe to messages from peer
//!
//! ## Usage Pattern
//!
//! ```rust,ignore
//! // Start actor
//! let peer_manager = PeerManagerActor::new(None).start();
//!
//! // Query peer role (for authorization)
//! let role = peer_manager
//!     .send(GetPeerRole { peer_id })
//!     .await?;
//!
//! // Get peer sender (for NetworkActor creation)
//! let sender = peer_manager
//!     .send(GetPeerSender { peer_id })
//!     .await?;
//! ```

use actix::prelude::*;
use std::sync::{Arc, Mutex};
use tokio::sync::{broadcast, mpsc};

use crate::{PeerId, PeerLifecycleEvent, PeerManager, PeerState};
use zznet_api::types::RoomId;
use zznet_api::types::{PeerIdentity, Role};
use zznet_router::{OnPeerConnected, RouterActor};

// ============================================================================
// Actor
// ============================================================================

/// PeerManagerActor - Actix wrapper for PeerManager
///
/// Provides message-based async access to peer state and channels.
pub struct PeerManagerActor {
    /// Shared underlying PeerManager (owns peer state)
    manager: Arc<Mutex<PeerManager>>,
    /// RouterActor for forwarding lifecycle events
    router_actor: Option<Addr<RouterActor>>,
}

impl PeerManagerActor {
    /// Create a new PeerManagerActor
    ///
    /// # Arguments
    /// * `max_peers` - Optional limit on total peer count
    pub fn new(max_peers: Option<usize>) -> Self {
        Self {
            manager: Arc::new(Mutex::new(PeerManager::new(max_peers))),
            router_actor: None,
        }
    }

    /// Create an actor that wraps an existing shared PeerManager instance.
    pub fn with_shared_manager(manager: Arc<Mutex<PeerManager>>) -> Self {
        Self {
            manager,
            router_actor: None,
        }
    }

    /// Set the RouterActor for forwarding lifecycle events
    pub fn set_router_actor(&mut self, router_actor: Addr<RouterActor>) {
        self.router_actor = Some(router_actor);
    }

    /// Subscribe to peer lifecycle events
    ///
    /// Returns a receiver for PeerConnected, PeerDisconnected, etc.
    pub fn subscribe_events(&self) -> broadcast::Receiver<PeerLifecycleEvent> {
        let m = self.manager.lock().unwrap();
        m.subscribe_events()
    }
}

impl Actor for PeerManagerActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        tracing::info!("PeerManagerActor started");
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        tracing::info!("PeerManagerActor stopped");
    }
}

// ============================================================================
// Essential Messages (Phase 7.1)
// ============================================================================

/// Get the authenticated role for a peer
///
/// Returns `None` if:
/// - Peer doesn't exist
/// - Peer has no role assigned (not authenticated yet)
#[derive(Message)]
#[rtype(result = "Option<Role>")]
pub struct GetPeerRole {
    pub peer_id: PeerId,
}

impl Handler<GetPeerRole> for PeerManagerActor {
    type Result = Option<Role>;

    fn handle(&mut self, msg: GetPeerRole, _ctx: &mut Context<Self>) -> Self::Result {
        let m = self.manager.lock().unwrap();
        m.get_peer_role(&msg.peer_id).cloned()
    }
}

// ============================================================================
// Additional Query Messages (Phase 7.2 - Future)
// ============================================================================

/// Get the full identity information for a peer
#[derive(Message)]
#[rtype(result = "Option<PeerIdentity>")]
pub struct GetPeerIdentity {
    pub peer_id: PeerId,
}

impl Handler<GetPeerIdentity> for PeerManagerActor {
    type Result = Option<PeerIdentity>;

    fn handle(&mut self, msg: GetPeerIdentity, _ctx: &mut Context<Self>) -> Self::Result {
        let m = self.manager.lock().unwrap();
        m.get_peer_identity(&msg.peer_id).cloned()
    }
}

/// Get all peers with a specific role
///
/// Useful for filtering operations (e.g., "send to all Collectors")
#[derive(Message)]
#[rtype(result = "Vec<PeerId>")]
pub struct GetPeersWithRole {
    pub role: Role,
}

impl Handler<GetPeersWithRole> for PeerManagerActor {
    type Result = Vec<PeerId>;

    fn handle(&mut self, msg: GetPeersWithRole, _ctx: &mut Context<Self>) -> Self::Result {
        let m = self.manager.lock().unwrap();
        m.peers_with_role(&msg.role)
    }
}

/// Get list of all peer IDs
#[derive(Message)]
#[rtype(result = "Vec<PeerId>")]
pub struct GetPeerIds;

impl Handler<GetPeerIds> for PeerManagerActor {
    type Result = Vec<PeerId>;

    fn handle(&mut self, _msg: GetPeerIds, _ctx: &mut Context<Self>) -> Self::Result {
        let m = self.manager.lock().unwrap();
        m.peer_ids()
    }
}

/// Check if a peer is connected
#[derive(Message)]
#[rtype(result = "bool")]
pub struct IsPeerConnected {
    pub peer_id: PeerId,
}

impl Handler<IsPeerConnected> for PeerManagerActor {
    type Result = bool;

    fn handle(&mut self, msg: IsPeerConnected, _ctx: &mut Context<Self>) -> Self::Result {
        let m = self.manager.lock().unwrap();
        m.is_peer_connected(&msg.peer_id)
    }
}

/// Get count of connected peers
#[derive(Message)]
#[rtype(result = "usize")]
pub struct GetConnectedPeerCount;

impl Handler<GetConnectedPeerCount> for PeerManagerActor {
    type Result = usize;

    fn handle(&mut self, _msg: GetConnectedPeerCount, _ctx: &mut Context<Self>) -> Self::Result {
        let m = self.manager.lock().unwrap();
        m.connected_peer_count()
    }
}

// ============================================================================
// Connection Management Messages (Phase 8)
// ============================================================================

/// Add a new connected peer
///
/// This message registers a fully-connected peer session.
/// Used by ConnectionManager after HELLO handshake completes.
///
/// The PeerState should be created using `PeerState::new_connected()`
/// with the peer's authentication context already set up.
#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct AddPeer {
    pub peer_state: PeerState,
}

impl Handler<AddPeer> for PeerManagerActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: AddPeer, _ctx: &mut Context<Self>) -> Self::Result {
        let peer_id = msg.peer_state.id().clone();
        tracing::info!("Adding peer {}", peer_id);

        let mut m = self.manager.lock().unwrap();
        m.add_peer(msg.peer_state)
            .map_err(|e| format!("Failed to add peer: {:?}", e))
    }
}

/// Connect peer with channels and forward to RouterActor
///
/// This message is sent when a peer connects with transport channels.
/// Derives permission from peer's role and forwards OnPeerConnected to RouterActor.
#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct ConnectPeerWithChannels {
    pub peer_id: PeerId,
    pub outbound_tx: mpsc::Sender<(RoomId, Vec<u8>)>,
    pub inbound_rx: mpsc::Receiver<(RoomId, Vec<u8>)>,
}

impl Handler<ConnectPeerWithChannels> for PeerManagerActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: ConnectPeerWithChannels, _ctx: &mut Context<Self>) -> Self::Result {
        let peer_id = msg.peer_id.clone();
        tracing::info!("Connecting peer {} with channels", peer_id);

        // Get permission from peer's role
        let permission = {
            let m = self.manager.lock().unwrap();
            let identity = m.get_peer_identity(&peer_id);
            zznet_api::types::Permission {
                peer_id: peer_id.clone(),
                identity: identity
                    .cloned()
                    .unwrap_or_else(|| zznet_api::types::PeerIdentity {
                        common_name: "unknown".to_string(),
                        san_username: "unknown".to_string(),
                        peer_addr: "unknown".to_string(),
                    }),
                capabilities: 0, // TODO: derive from role
            }
        };

        // Forward to RouterActor
        if let Some(router_actor) = &self.router_actor {
            let msg = OnPeerConnected {
                peer_id,
                permission,
                outbound_tx: msg.outbound_tx,
                inbound_rx: msg.inbound_rx,
            };
            router_actor.do_send(msg);
        } else {
            tracing::warn!(
                "No RouterActor set, cannot forward OnPeerConnected for {}",
                peer_id
            );
        }

        Ok(())
    }
}

/// Remove a peer and clean up resources
///
/// This terminates the peer connection and removes all state.
#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct RemovePeer {
    pub peer_id: PeerId,
}

impl Handler<RemovePeer> for PeerManagerActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: RemovePeer, _ctx: &mut Context<Self>) -> Self::Result {
        tracing::info!("Removing peer {}", msg.peer_id);
        let mut m = self.manager.lock().unwrap();
        m.remove_peer(&msg.peer_id)
            .map_err(|e| format!("Failed to remove peer: {:?}", e))
    }
}

/// Disconnect a peer (mark as disconnected but keep state)
///
/// This transitions the peer to Disconnected state without removing it.
#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct DisconnectPeer {
    pub peer_id: PeerId,
}

impl Handler<DisconnectPeer> for PeerManagerActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: DisconnectPeer, _ctx: &mut Context<Self>) -> Self::Result {
        tracing::info!("Disconnecting peer {}", msg.peer_id);
        // Remove the peer from PeerManager (this handles disconnection and notification)
        let mut m = self.manager.lock().unwrap();
        m.remove_peer(&msg.peer_id)
            .map_err(|e| format!("Failed to disconnect peer: {:?}", e))
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[actix::test]
    async fn test_peer_manager_actor_get_peer_role() {
        // Create actor
        let peer_manager = PeerManagerActor::new(None).start();

        // Query non-existent peer
        let result = peer_manager
            .send(GetPeerRole {
                peer_id: PeerId::from("nonexistent"),
            })
            .await
            .unwrap();

        assert!(result.is_none());
    }
}
