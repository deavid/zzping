use zznet_api::types::{ConnectionState, PeerId, PeerIdentity, PeerStateMut, PeerStateView, Role};

/// Control-plane representation of a peer.
///
/// Maintains lifecycle and authentication context without any data-plane
/// responsibilities. This decouples peer state tracking from transport
/// channel management.
#[derive(Debug, Clone)]
pub struct PeerState {
    peer_id: PeerId,
    connection_state: ConnectionState,
    role: Option<Role>,
    identity: Option<PeerIdentity>,
}

impl PeerState {
    /// Create a new peer in the disconnected state with no auth context.
    pub fn new(peer_id: PeerId) -> Self {
        Self {
            peer_id,
            connection_state: ConnectionState::Disconnected,
            role: None,
            identity: None,
        }
    }

    /// Convenience helper for constructing a connected peer with auth context.
    pub fn new_connected(
        peer_id: PeerId,
        role: Option<Role>,
        identity: Option<PeerIdentity>,
    ) -> Self {
        Self {
            peer_id,
            connection_state: ConnectionState::Connected,
            role,
            identity,
        }
    }

    /// Borrow the identifier for this peer.
    pub fn id(&self) -> &PeerId {
        &self.peer_id
    }

    /// Borrow the authenticated role, if available.
    pub fn role(&self) -> Option<&Role> {
        self.role.as_ref()
    }

    /// Borrow the authenticated identity, if available.
    pub fn identity(&self) -> Option<&PeerIdentity> {
        self.identity.as_ref()
    }
}

impl PeerStateView for PeerState {
    fn peer_id(&self) -> &PeerId {
        &self.peer_id
    }

    fn connection_state(&self) -> ConnectionState {
        self.connection_state
    }

    fn role(&self) -> Option<&Role> {
        self.role.as_ref()
    }

    fn identity(&self) -> Option<&PeerIdentity> {
        self.identity.as_ref()
    }
}

impl PeerStateMut for PeerState {
    fn set_connection_state(&mut self, state: ConnectionState) {
        self.connection_state = state;
    }

    fn set_role(&mut self, role: Option<Role>) {
        self.role = role;
    }

    fn set_identity(&mut self, identity: Option<PeerIdentity>) {
        self.identity = identity;
    }
}
