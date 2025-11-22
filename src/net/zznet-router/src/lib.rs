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
mod error;
mod factory_utils;
mod messages;
mod room_factory;
mod router;

pub use actor::{RegisterManager, RouterActor};
pub use factory_utils::{NetworkComponent, StandardRoomFactory};
pub use messages::RegisterPeer;
pub use room_factory::{RoomFactory, RoomFactoryRef};
