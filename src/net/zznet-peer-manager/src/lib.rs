//! Peer manager for ZZNet router.
//!
//! This crate provides the peer management functionality for the ZZNet router,
//! handling peer connections, disconnections, and state management.

mod actor;
mod peer_manager;
mod peer_state;

pub use actor::{
    AddPeer, ConnectPeerWithChannels, DisconnectPeer, GetConnectedPeerCount, GetPeerIdentity,
    GetPeerIds, GetPeerRole, GetPeersWithRole, IsPeerConnected, PeerManagerActor, RemovePeer,
};
pub use peer_state::PeerState;
