//! zznet-router
//!
//! Data-plane router: manages per-peer channels, room membership/negotiation, and byte routing.
//!
//! Allowed responsibilities:
//! - Channel registration (peer -> sender/receiver)
//! - Room membership and negotiation (PublishRooms)
//! - Byte routing to room handlers
//! - Inbound/outbound broadcast/send operations
//!
//! NOT allowed:
//! - Role queries or auth checks
//! - Peer identity or lifecycle state
//! - Business logic
//!
//! This crate provides direct message routing via actor messages and must not import
//! business/auth types such as `Role` or `PeerIdentity`.

mod actor;
mod peer_channels;
mod router;

pub use actor::{
    GetOfferedRooms, OnPeerConnected, OnPeerDisconnected, RegisterManager, RouterActor,
};
pub use peer_channels::PeerChannels;
