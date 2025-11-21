//! Factory utilities for the NetworkComponent pattern.
//!
//! This module provides the NetworkComponent trait and StandardRoomFactory implementation
//! that eliminate boilerplate code across components.

use crate::messages::RegisterPeer;
use crate::room_factory::RoomFactory;
use actix::prelude::*;
use std::collections::HashMap;
use zznet_api::types::{PeerId, Role, RoomId};
use zznet_room::actor::RoomActor;
use zznet_room::room_manager::RoomInboundRecipient;
use zznet_room::room_message_trait::RoomMessageTrait;

/// NetworkComponent trait - defines the universe of types for a component.
///
/// This trait acts as a type family (manifest) that binds together all the types
/// needed for a network-enabled component. By implementing this trait, a component
/// can use the StandardRoomFactory and generic RegisterPeer message, eliminating
/// significant boilerplate code.
///
/// # Type Safety
///
/// The associated types ensure that:
/// - The NetworkActor can handle the ProtocolMessage
/// - The ManagerActor can handle RegisterPeer for this component
/// - All types are properly bound together and can't be mixed incorrectly
///
/// # Zero-Sized Types
///
/// Implementations should be Zero-Sized Types (ZSTs) for zero runtime overhead:
/// ```ignore
/// #[derive(Clone)]
/// pub struct MyComponentManifest;
/// impl NetworkComponent for MyComponentManifest { ... }
/// ```
pub trait NetworkComponent: 'static + Sized + Send + Sync + Clone {
    /// The unique room identifier for this component
    const ROOM_ID: &'static str;

    /// The MainActor that handles business logic
    type MainActor: Actor<Context = Context<Self::MainActor>>;

    /// The protocol message type exchanged over the network
    type ProtocolMessage: RoomMessageTrait + Message<Result = ()>;

    /// The NetworkActor that handles per-peer protocol translation
    type NetworkActor: Actor<Context = Context<Self::NetworkActor>> + Handler<Self::ProtocolMessage>;

    /// The NetworkManager that handles peer lifecycle and registration
    type ManagerActor: Actor<Context = Context<Self::ManagerActor>> + Handler<RegisterPeer<Self>>;

    /// Component-specific permissions type
    type Permissions: Clone + Default + Send + Sync;

    /// Factory method to create a NetworkActor instance.
    ///
    /// This method acts as an adapter - it allows components to have different
    /// constructor signatures while providing a uniform interface. For example:
    /// - Some NetworkActors may not need the manager address
    /// - Some may need additional context
    ///
    /// The implementation simply calls the component's NetworkActor constructor
    /// with the appropriate arguments, ignoring unused parameters if necessary.
    fn create_network_actor(
        peer_id: PeerId,
        perms: Self::Permissions,
        main: Addr<Self::MainActor>,
        mgr: Addr<Self::ManagerActor>,
    ) -> Self::NetworkActor;
}

/// Standard RoomFactory implementation for any NetworkComponent.
///
/// This generic factory eliminates the need for each component to implement
/// its own factory. It handles:
/// - Room ID validation
/// - Role-to-Permissions translation
/// - Synchronous actor spawning
/// - Fire-and-forget peer registration
///
/// The factory holds a ZST instance of the manifest for cleaner API
/// (no PhantomData needed).
pub struct StandardRoomFactory<C: NetworkComponent> {
    /// The manifest instance (Zero-Sized Type, no runtime overhead)
    _manifest: C,
    /// Address of the MainActor
    main_actor: Addr<C::MainActor>,
    /// Address of the NetworkManager (for registration)
    manager_actor: Addr<C::ManagerActor>,
    /// Permissions map for role-to-permissions translation
    permissions: HashMap<String, C::Permissions>,
}

impl<C: NetworkComponent> StandardRoomFactory<C> {
    /// Create a new StandardRoomFactory.
    ///
    /// # Arguments
    /// * `manifest` - The ZST manifest instance (e.g., `MyComponentManifest`)
    /// * `main` - Address of the MainActor
    /// * `mgr` - Address of the NetworkManager
    /// * `perms` - Map from role strings to component permissions
    pub fn new(
        manifest: C,
        main: Addr<C::MainActor>,
        mgr: Addr<C::ManagerActor>,
        perms: HashMap<String, C::Permissions>,
    ) -> Self {
        Self {
            _manifest: manifest,
            main_actor: main,
            manager_actor: mgr,
            permissions: perms,
        }
    }
}

impl<C: NetworkComponent> RoomFactory for StandardRoomFactory<C> {
    fn create_room(
        &self,
        peer_id: PeerId,
        role: Role,
        room_id: RoomId,
        outbound_to_peer: tokio::sync::mpsc::Sender<(RoomId, Vec<u8>)>,
    ) -> Result<Option<RoomInboundRecipient>, String> {
        // 1. Check Room ID - return None if this isn't our room
        if room_id.as_str() != C::ROOM_ID {
            return Ok(None);
        }

        // 2. Lookup Permissions (use default if role not found)
        let perms = self
            .permissions
            .get(role.as_str())
            .cloned()
            .unwrap_or_default();

        // 3. Spawn Network Actor (delegated to trait method)
        let net = C::create_network_actor(
            peer_id.clone(),
            perms,
            self.main_actor.clone(),
            self.manager_actor.clone(),
        );
        let net_addr = net.start();

        // 4. Spawn Room Actor (standard serialization/deserialization layer)
        let room = RoomActor::new(
            room_id,
            outbound_to_peer,
            net_addr.clone().recipient::<C::ProtocolMessage>(),
        );
        let room_addr = room.start();

        // 5. Fire-and-Forget Register with Manager
        self.manager_actor.do_send(RegisterPeer::<C> {
            peer_id,
            network_actor: net_addr,
            room_actor: room_addr.clone(),
        });

        // 6. Return the recipient immediately
        Ok(Some(room_addr.recipient()))
    }
}
