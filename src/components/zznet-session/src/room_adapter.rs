//! Room adapter for type erasure
//!
//! This module provides the bridge between concrete `Room<T>` types and the
//! type-erased `RoomHandle<TMsg>` trait, enabling PeerSession to store rooms
//! with different component message types in a single collection.

use crate::peer_session::RoomHandle;
use crate::room_message_trait::RoomMessageTrait;
use crate::types::{RoomId, SessionError};
use tokio::sync::mpsc;

/// Adapter that wraps a Room<T> and implements RoomHandle<TMsg>
///
/// This enables type erasure: different Room<T> types can be stored as
/// Box<dyn RoomHandle<TMsg>> in PeerSession's HashMap.
///
/// # Type Parameters
///
/// - `T`: The concrete component message type (e.g., IntentConfigMessage)
/// - `TMsg`: The application's message enum type (e.g., CollectorMessages)
///
/// # Requirements
///
/// - `T: TryFrom<TMsg>` - Extract T from application enum
/// - `T: Into<TMsg>` - Wrap T in application enum
/// - `T: Message` - Actix message type
///
/// # Example
///
/// ```rust,ignore
/// // Create room with concrete type
/// let actor = IntentConfigActor::new(...).start();
/// let (room, channels) = Room::<IntentConfigMessage>::new(actor.recipient());
///
/// // Wrap in adapter for type erasure
/// let (tx, _rx) = mpsc::channel(10);
/// let adapter = RoomAdapter::new(
///     RoomId::from("intentconfig"),
///     channels.inbound_tx,
///     channels.outbound_rx,
///     tx,
/// );
///
/// // Spawn room receiver
/// room.spawn_receiver()?;
///
/// // Type-erase to trait object
/// let boxed: Box<dyn RoomHandle<CollectorMessages>> = Box::new(adapter);
///
/// // Can now store in HashMap with other room types
/// peer_session.add_room(room_id, boxed)?;
/// ```
pub struct RoomAdapter<T, TMsg>
where
    T: Send + Clone + TryFrom<TMsg> + Into<TMsg> + 'static,
    TMsg: RoomMessageTrait,
{
    room_id: RoomId,
    // Channel to send inbound messages to the room
    inbound_tx: mpsc::Sender<T>,
    // Task that forwards outbound messages
    forwarder_task: Option<tokio::task::JoinHandle<()>>,
    _phantom: std::marker::PhantomData<TMsg>,
}

impl<T, TMsg> RoomAdapter<T, TMsg>
where
    T: Send + Clone + TryFrom<TMsg> + Into<TMsg> + 'static,
    TMsg: RoomMessageTrait,
{
    /// Create a new room adapter
    ///
    /// Takes the room's channels and wraps them for use with the application's
    /// message enum. The Room itself must be spawned separately via spawn_receiver().
    ///
    /// # Arguments
    ///
    /// - `room_id`: The room identifier (e.g., "intentconfig")
    /// - `inbound_tx`: Channel to send inbound messages to the room
    /// - `outbound_rx`: Channel to receive outbound messages from the room
    /// - `peer_tx`: Channel to send messages to the peer (wrapped in TMsg)
    pub fn new(
        room_id: RoomId,
        inbound_tx: mpsc::Sender<T>,
        mut outbound_rx: mpsc::Receiver<T>,
        peer_tx: mpsc::Sender<(RoomId, TMsg)>,
    ) -> Self {
        // Spawn task to forward outbound messages
        let room_id_clone = room_id.clone();
        let task = tokio::spawn(async move {
            while let Some(msg) = outbound_rx.recv().await {
                // Convert T → TMsg
                let app_msg: TMsg = msg.into();

                // Send to peer
                if peer_tx
                    .send((room_id_clone.clone(), app_msg))
                    .await
                    .is_err()
                {
                    tracing::warn!("Room {} outbound channel closed", room_id_clone);
                    break;
                }
            }
            tracing::debug!("Room {} forwarder task stopped", room_id_clone);
        });

        Self {
            room_id,
            inbound_tx,
            forwarder_task: Some(task),
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<T, TMsg> RoomHandle<TMsg> for RoomAdapter<T, TMsg>
where
    T: Send + Clone + TryFrom<TMsg> + Into<TMsg> + 'static,
    TMsg: RoomMessageTrait,
{
    fn room_id(&self) -> &RoomId {
        &self.room_id
    }

    fn send_message(&mut self, msg: TMsg) -> Result<(), SessionError> {
        // Convert TMsg → T
        let concrete: T = msg.try_into().map_err(|_| SessionError::WrongMessageType)?;

        // Send to room's inbound channel
        // Use try_send to avoid blocking (room might be processing)
        self.inbound_tx
            .try_send(concrete)
            .map_err(|_| SessionError::SendFailed)?;

        Ok(())
    }

    fn spawn_forwarder(&mut self, _tx: mpsc::Sender<(RoomId, TMsg)>) -> Result<(), SessionError> {
        // Forwarder is already spawned in new()
        // This method is called by PeerSession::connect(), but we handle it in constructor
        // Just return Ok if already spawned
        if self.forwarder_task.is_some() {
            Ok(())
        } else {
            Err(SessionError::SendFailed)
        }
    }
}

impl<T, TMsg> Drop for RoomAdapter<T, TMsg>
where
    T: Send + Clone + TryFrom<TMsg> + Into<TMsg> + 'static,
    TMsg: RoomMessageTrait,
{
    fn drop(&mut self) {
        // Abort forwarder task when adapter is dropped
        if let Some(task) = self.forwarder_task.take() {
            task.abort();
        }
    }
}

// Tests will be added in integration tests where we have concrete Message types
// Unit tests here are difficult because IntentConfigMessage doesn't implement actix::Message
// in the test_room_messages module (it's just for serialization testing)
