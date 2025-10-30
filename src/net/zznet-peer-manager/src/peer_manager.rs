use std::collections::HashMap;
use tokio::sync::broadcast;
use zznet_api::types::{
    ConnectionState, PeerId, PeerIdentity, PeerLifecycleEvent, PeerStateMut, PeerStateView, Role,
    SessionError,
};

use crate::peer_state::PeerState;

/// PeerManager - Control Plane for peer state and lifecycle
///
/// **Responsibilities**:
/// - Peer HashMap management (add/remove/query)
/// - Enforcing max_peers limit
/// - Tracking peer identity and roles
/// - Broadcasting lifecycle events
///
/// **Non-Responsibilities** (handled by Router):
/// - Room management and routing
/// - Message sending
/// - offered_rooms negotiation
pub struct PeerManager {
    /// All peer sessions (key responsibility: state management)
    peers: HashMap<PeerId, PeerState>,

    /// Optional maximum number of peers
    max_peers: Option<usize>,

    /// Event broadcaster for lifecycle events
    /// Components can subscribe to receive PeerConnected/Disconnected notifications
    event_tx: broadcast::Sender<PeerLifecycleEvent>,
}

impl PeerManager {
    /// Create a new PeerManager
    ///
    /// # Arguments
    /// * `max_peers` - Optional limit on total peer count
    ///
    /// # Example
    /// ```
    /// use zznet_peer_manager::PeerManager;
    ///
    /// // Unlimited peers
    /// let pm = PeerManager::new(None);
    ///
    /// // Limited to 100 peers
    /// let pm_limited = PeerManager::new(Some(100));
    /// ```
    pub fn new(max_peers: Option<usize>) -> Self {
        let (event_tx, _) = broadcast::channel(100);

        tracing::info!("PeerManager created with max_peers = {:?}", max_peers);

        Self {
            peers: HashMap::new(),
            max_peers,
            event_tx,
        }
    }

    /// Subscribe to peer lifecycle events
    ///
    /// Returns a receiver that will get PeerConnected, PeerDisconnected, etc.
    ///
    /// # Example
    /// ```no_run
    /// use zznet_peer_manager::PeerManager;
    ///
    /// let pm = PeerManager::new(None);
    /// let mut events = pm.subscribe_events();
    ///
    /// // In your async task:
    /// // while let Ok(event) = events.recv().await {
    /// //     println!("Peer event: {:?}", event);
    /// // }
    /// ```
    pub fn subscribe_events(&self) -> broadcast::Receiver<PeerLifecycleEvent> {
        self.event_tx.subscribe()
    }

    /// Get the configured max_peers limit
    pub fn max_peers(&self) -> Option<usize> {
        self.max_peers
    }

    /// Add a new peer session
    ///
    /// Validates against max_peers limit and broadcasts PeerAdded event.
    ///
    /// # Errors
    /// - `SessionError::PeerAlreadyExists` if peer_id already registered
    /// - `SessionError::PeerLimitExceeded` if max_peers limit reached
    pub fn add_peer(&mut self, peer_state: PeerState) -> Result<(), SessionError> {
        let peer_id = peer_state.id().clone();

        // Check if peer already exists
        if self.peers.contains_key(&peer_id) {
            return Err(SessionError::PeerAlreadyExists(peer_id));
        }

        // Enforce max_peers limit
        if let Some(max) = self.max_peers
            && self.peers.len() >= max
        {
            return Err(SessionError::PeerLimitExceeded { max });
        }

        // Add peer
        self.peers.insert(peer_id.clone(), peer_state);

        // Broadcast event (ignore send errors - no subscribers yet is OK)
        let _ = self.event_tx.send(PeerLifecycleEvent::PeerAdded {
            peer_id: peer_id.clone(),
        });

        tracing::info!("Peer added: {}", peer_id);
        Ok(())
    }

    /// Remove a peer entirely
    ///
    /// Disconnects and removes from map, broadcasts PeerRemoved event.
    ///
    /// # Errors
    /// - `SessionError::PeerNotFound` if peer_id not registered
    pub fn remove_peer(&mut self, peer_id: &PeerId) -> Result<(), SessionError> {
        self.peers
            .remove(peer_id)
            .ok_or_else(|| SessionError::PeerNotFound(peer_id.clone()))?;

        // Broadcast event
        let _ = self.event_tx.send(PeerLifecycleEvent::PeerRemoved {
            peer_id: peer_id.clone(),
        });

        tracing::info!("Peer removed: {}", peer_id);
        Ok(())
    }

    /// Get mutable reference to a peer (for connection operations)
    pub fn get_peer_mut(&mut self, peer_id: &PeerId) -> Option<&mut PeerState> {
        self.peers.get_mut(peer_id)
    }

    /// Get immutable reference to a peer
    pub fn get_peer(&self, peer_id: &PeerId) -> Option<&PeerState> {
        self.peers.get(peer_id)
    }

    /// Get the connection state of a peer
    pub fn peer_state(&self, peer_id: &PeerId) -> Option<ConnectionState> {
        self.peers.get(peer_id).map(|p| p.connection_state())
    }

    /// Check if a peer is connected
    pub fn is_peer_connected(&self, peer_id: &PeerId) -> bool {
        self.peers
            .get(peer_id)
            .map(|p| p.connection_state() == ConnectionState::Connected)
            .unwrap_or(false)
    }

    /// Get list of all peer IDs
    pub fn peer_ids(&self) -> Vec<PeerId> {
        self.peers.keys().cloned().collect()
    }

    /// Get the authenticated role for a peer
    pub fn get_peer_role(&self, peer_id: &PeerId) -> Option<&Role> {
        self.peers.get(peer_id)?.role()
    }

    /// Get the full identity information for a peer
    pub fn get_peer_identity(&self, peer_id: &PeerId) -> Option<&PeerIdentity> {
        self.peers.get(peer_id)?.identity()
    }

    /// Get all peers with a specific role
    pub fn peers_with_role(&self, role: &Role) -> Vec<PeerId> {
        self.peers
            .iter()
            .filter_map(|(id, session)| {
                if session.role() == Some(role) {
                    Some(id.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get number of connected peers
    pub fn connected_peer_count(&self) -> usize {
        self.peers
            .values()
            .filter(|p| p.connection_state() == ConnectionState::Connected)
            .count()
    }

    /// Notify that a peer connected (for event broadcasting)
    ///
    /// Called by SessionManager when a peer transitions to Connected state
    pub fn notify_peer_connected(&mut self, peer_id: &PeerId) {
        if let Some(peer) = self.peers.get_mut(peer_id) {
            peer.set_connection_state(ConnectionState::Connected);
        }
        let _ = self.event_tx.send(PeerLifecycleEvent::PeerConnected {
            peer_id: peer_id.clone(),
        });
        tracing::debug!("PeerConnected event broadcast for {}", peer_id);
    }

    /// Notify that a peer disconnected (for event broadcasting)
    ///
    /// Called by SessionManager when a peer transitions away from Connected state
    pub fn notify_peer_disconnected(&mut self, peer_id: &PeerId) {
        if let Some(peer) = self.peers.get_mut(peer_id) {
            peer.set_connection_state(ConnectionState::Disconnected);
        }
        let _ = self.event_tx.send(PeerLifecycleEvent::PeerDisconnected {
            peer_id: peer_id.clone(),
        });
        tracing::debug!("PeerDisconnected event broadcast for {}", peer_id);
    }

    /// Notify that a peer's identity was updated (for event broadcasting)
    ///
    /// Called after authentication completes
    pub fn notify_peer_identity_updated(&mut self, peer_id: &PeerId, identity: PeerIdentity) {
        if let Some(peer) = self.peers.get_mut(peer_id) {
            peer.set_identity(Some(identity.clone()));
        }
        let _ = self.event_tx.send(PeerLifecycleEvent::PeerIdentityUpdated {
            peer_id: peer_id.clone(),
            identity,
        });
        tracing::debug!("PeerIdentityUpdated event broadcast for {}", peer_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_peer_manager_add_remove() {
        let mut pm = PeerManager::new(None);

        // Create a test peer state (connected)
        let peer_id = PeerId::from("test-peer");
        let peer_state = PeerState::new_connected(peer_id.clone(), None, None);

        // Add peer
        pm.add_peer(peer_state).unwrap();
        assert_eq!(pm.peer_ids().len(), 1);
        assert!(pm.get_peer(&peer_id).is_some());

        // Remove peer
        pm.remove_peer(&peer_id).unwrap();
        assert_eq!(pm.peer_ids().len(), 0);
        assert!(pm.get_peer(&peer_id).is_none());
    }

    #[test]
    fn test_max_peers_enforcement() {
        let mut pm = PeerManager::new(Some(2));

        // Add first peer - OK
        pm.add_peer(PeerState::new_connected(PeerId::from("peer1"), None, None))
            .unwrap();

        // Add second peer - OK
        pm.add_peer(PeerState::new_connected(PeerId::from("peer2"), None, None))
            .unwrap();

        // Add third peer - Should fail
        let result = pm.add_peer(PeerState::new_connected(PeerId::from("peer3"), None, None));

        assert!(matches!(
            result,
            Err(SessionError::PeerLimitExceeded { max: 2 })
        ));
    }

    #[test]
    fn test_lifecycle_event_broadcasting() {
        let mut pm = PeerManager::new(None);
        let mut event_rx = pm.subscribe_events();

        let peer_id = PeerId::from("test-peer");
        let peer_state = PeerState::new_connected(peer_id.clone(), None, None);

        // Add peer - should broadcast PeerAdded
        pm.add_peer(peer_state).unwrap();

        let event = event_rx.try_recv().unwrap();
        assert!(matches!(event, PeerLifecycleEvent::PeerAdded { .. }));

        // Manually notify connected
        pm.notify_peer_connected(&peer_id);
        let event = event_rx.try_recv().unwrap();
        assert!(matches!(event, PeerLifecycleEvent::PeerConnected { .. }));

        // Remove peer - should broadcast PeerRemoved
        pm.remove_peer(&peer_id).unwrap();
        let event = event_rx.try_recv().unwrap();
        assert!(matches!(event, PeerLifecycleEvent::PeerRemoved { .. }));
    }

    #[test]
    fn test_peer_already_exists() {
        let mut pm = PeerManager::new(None);
        let peer_id = PeerId::from("duplicate");

        // Add first time - OK
        pm.add_peer(PeerState::new_connected(peer_id.clone(), None, None))
            .unwrap();

        // Add second time - Should fail
        let result = pm.add_peer(PeerState::new_connected(peer_id.clone(), None, None));
        assert!(matches!(result, Err(SessionError::PeerAlreadyExists(_))));
    }
}
