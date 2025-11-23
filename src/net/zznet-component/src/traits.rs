//! The `NetComponent` trait - contract for components using the generic system.

use crate::messages::SetRoomActor;
use actix::prelude::*;

/// Trait that defines the types and behavior of a network component.
///
/// Components implement this trait to specify their business logic actor,
/// network message types, events, and how to construct their network actors.
/// The generic machinery (`GenericNetworkManager`, `GenericRoomFactory`) uses
/// this information to automatically handle all the boilerplate.
///
/// # Example
///
/// ```ignore
/// pub struct IntentConfigSpec;
///
/// impl NetComponent for IntentConfigSpec {
///     const ROOM_ID: &'static str = "intent-config";
///     type MainActor = IntentConfigActor;
///     type NetworkMsg = IntentConfigNetworkMsg;
///     type Event = IntentConfigEvent;
///     type Permissions = IntentConfigPermissions;
///     type NetworkActor = IntentConfigNetworkActor;
///
///     fn build_network_actor(
///         peer_id: zznet_api::PeerId,
///         permissions: Self::Permissions,
///         main_actor: Addr<Self::MainActor>,
///         event_rx: tokio::sync::broadcast::Receiver<Self::Event>,
///     ) -> Self::NetworkActor {
///         IntentConfigNetworkActor::new(peer_id, permissions, main_actor, event_rx)
///     }
/// }
/// ```
pub trait NetComponent: Sized + 'static {
    /// The static Room ID string (e.g., "intent-config").
    ///
    /// This identifies which room this component handles.
    const ROOM_ID: &'static str;

    /// The business logic actor type.
    ///
    /// This is the main actor that implements the component's core functionality.
    type MainActor: Actor;

    /// The network message enum (must implement RoomMessageTrait).
    ///
    /// These are the typed messages exchanged over the network for this component.
    type NetworkMsg: zznet_room::RoomMessageTrait + Message<Result = ()> + Send;

    /// The event type broadcast by MainActor.
    ///
    /// These events are published by the MainActor when state changes occur,
    /// and NetworkActors subscribe to them to notify their peers.
    type Event: Message + Clone + Send + Unpin;

    /// The permission struct used by the NetworkActor.
    ///
    /// This defines what operations a peer is allowed to perform.
    type Permissions: Clone + Send + Sync + Default + Unpin;

    /// The per-peer translator actor.
    ///
    /// This actor translates between network messages and domain messages.
    /// It must handle the component's NetworkMsg and the SetRoomActor message.
    /// The actor must use actix::Context as its context type.
    type NetworkActor: Actor<Context = actix::Context<Self::NetworkActor>>
        + Handler<Self::NetworkMsg>
        + Handler<SetRoomActor<Self::NetworkMsg>>;

    /// Constructor for the NetworkActor.
    ///
    /// This is called by the factory when a new peer joins the room.
    ///
    /// # Arguments
    /// - `peer_id`: The ID of the connecting peer
    /// - `permissions`: The permissions granted to this peer
    /// - `main_actor`: Address of the MainActor for forwarding domain messages
    /// - `event_rx`: Receiver for subscribing to events from the MainActor
    fn build_network_actor(
        peer_id: zznet_api::PeerId,
        permissions: Self::Permissions,
        main_actor: Addr<Self::MainActor>,
        event_rx: tokio::sync::broadcast::Receiver<Self::Event>,
    ) -> Self::NetworkActor;
}
