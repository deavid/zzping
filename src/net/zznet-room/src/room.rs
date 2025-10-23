use actix::prelude::*;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// A bidirectional typed communication channel between two components.
///
/// A `Room<T>` allows a local component to send and receive typed messages
/// of type `T` to/from a peer component. It handles serialization/deserialization
/// automatically and provides channels for connecting to a SessionManager.
///
pub struct Room<T>
where
    T: Message<Result = ()> + Send + Clone + Serialize + for<'de> Deserialize<'de> + 'static,
{
    room_id: String,
    // Send serialized messages to peer (outbound - now Vec<u8>)
    outbound_tx: mpsc::Sender<Vec<u8>>,

    // Receive serialized messages from peer (inbound - now Vec<u8>)
    inbound_rx: Option<mpsc::Receiver<Vec<u8>>>,

    // Local component that handles received messages
    local_handler: Recipient<T>,

    // Background task that forwards inbound → handler
    receiver_task: Option<JoinHandle<()>>,
}

/// Channels returned when creating a Room, used for connecting to SessionManager
/// These channels now carry serialized bytes instead of typed messages
pub struct RoomChannels {
    /// Receiver for outbound messages (connect to SessionManager's inbound)
    /// Messages are already serialized to Vec<u8>
    pub outbound_rx: mpsc::Receiver<Vec<u8>>,
    /// Sender for inbound messages (connect to SessionManager's outbound)
    /// Expects serialized Vec<u8> that will be deserialized to T
    pub inbound_tx: mpsc::Sender<Vec<u8>>,
}

#[derive(Debug, Error)]
/// Error returned when processing inbound messages fails.
pub enum ProcessError {
    #[error("Receiver already spawned in background")]
    /// The room receiver task was already spawned and cannot be used manually.
    ReceiverAlreadySpawned,
    #[error("Handler failed to process message")]
    /// The local handler failed to process the forwarded message.
    HandlerFailed,
    #[error("Channel closed")]
    /// The inbound channel was closed and no more messages can be received.
    ChannelClosed,
    #[error("Deserialization failed: {0}")]
    /// Failed to deserialize incoming message.
    DeserializationFailed(String),
}

#[derive(Debug, Error)]
/// Error returned when attempting to spawn the room receiver task.
pub enum SpawnError {
    #[error("Receiver already spawned")]
    /// Attempted to spawn the receiver when it was already running.
    AlreadySpawned,
}

#[derive(Debug, Error)]
/// Error returned when sending a message fails.
pub enum SendError {
    #[error("Serialization failed: {0}")]
    /// Failed to serialize outgoing message.
    SerializationFailed(String),
    #[error("Channel send failed")]
    /// The channel is closed or disconnected.
    ChannelClosed,
}

impl<T> Room<T>
where
    T: Message<Result = ()> + Send + Clone + Serialize + for<'de> Deserialize<'de> + 'static,
{
    /// Create a new Room with the given room ID and local handler
    /// Returns the Room and channels for connecting to SessionManager
    pub fn new(room_id: String, local_handler: Recipient<T>) -> (Self, RoomChannels) {
        let (outbound_tx, outbound_rx) = mpsc::channel(100);
        let (inbound_tx, inbound_rx) = mpsc::channel(100);

        let room = Room {
            room_id,
            outbound_tx,
            inbound_rx: Some(inbound_rx),
            local_handler,
            receiver_task: None,
        };

        let channels = RoomChannels {
            outbound_rx,
            inbound_tx,
        };

        (room, channels)
    }

    /// Send a typed message to the peer via this room
    /// The message will be serialized and sent through the outbound channel
    pub async fn send(&self, msg: T) -> Result<(), SendError> {
        // Serialize the message to bytes using serde via bincode
        let bytes = bincode::serde::encode_to_vec(&msg, bincode::config::standard())
            .map_err(|e| SendError::SerializationFailed(e.to_string()))?;

        // Send serialized bytes through the channel
        self.outbound_tx
            .send(bytes)
            .await
            .map_err(|_| SendError::ChannelClosed)?;

        Ok(())
    }

    /// Get a cloneable handle for sending messages to this room
    ///
    /// This is useful when you need to send messages from async contexts that
    /// outlive the Room's immediate scope (e.g., spawned tasks in Actix handlers).
    /// Since `mpsc::Sender<Vec<u8>>` implements Clone, multiple senders can share
    /// the same channel safely.
    pub fn sender(&self) -> mpsc::Sender<Vec<u8>> {
        self.outbound_tx.clone()
    }

    /// Process one inbound message manually (for testing)
    /// Returns Ok(true) if message was processed
    /// Returns Ok(false) if no message available
    /// Returns Err if channel is closed
    #[deprecated(note = "This function has either to be removed or be called in spawn_receiver")]
    pub async fn process_one(&mut self) -> Result<bool, ProcessError> {
        let rx = self
            .inbound_rx
            .as_mut()
            .ok_or(ProcessError::ReceiverAlreadySpawned)?;

        match rx.try_recv() {
            Ok(msg) => {
                Self::handle_message(&self.local_handler, msg).await?;
                Ok(true)
            }
            Err(mpsc::error::TryRecvError::Empty) => Ok(false),
            Err(mpsc::error::TryRecvError::Disconnected) => Err(ProcessError::ChannelClosed),
        }
    }

    /// Spawn a background task that automatically forwards inbound messages
    /// to the local handler. Can only be called once.
    pub fn spawn_receiver(&mut self) -> Result<(), SpawnError> {
        if self.receiver_task.is_some() {
            return Err(SpawnError::AlreadySpawned);
        }

        let mut rx = self.inbound_rx.take().ok_or(SpawnError::AlreadySpawned)?;
        let handler = self.local_handler.clone();

        let task = tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if let Err(e) = Self::handle_message(&handler, msg).await {
                    tracing::error!("Error receiving message for room: {e:?}");
                }
            }
            tracing::debug!("Room receiver task stopped");
        });

        self.receiver_task = Some(task);
        Ok(())
    }

    /// Get the room ID
    pub fn room_id(&self) -> &str {
        &self.room_id
    }

    /// Internal helper to handle a single message by deserializing and forwarding to the handler
    async fn handle_message(handler: &Recipient<T>, bytes: Vec<u8>) -> Result<(), ProcessError> {
        // Deserialize the bytes to T using serde via bincode
        let (msg, _): (T, _) =
            bincode::serde::decode_from_slice(&bytes, bincode::config::standard())
                .map_err(|e| ProcessError::DeserializationFailed(e.to_string()))?;

        // Forward to handler
        handler
            .send(msg)
            .await
            .map_err(|_| ProcessError::HandlerFailed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Message, Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
    #[rtype(result = "()")]
    struct TestMsg {
        value: i32,
    }

    struct TestActor {
        received: Vec<TestMsg>,
    }

    impl Actor for TestActor {
        type Context = Context<Self>;
    }

    impl Handler<TestMsg> for TestActor {
        type Result = ();
        fn handle(&mut self, msg: TestMsg, _: &mut Context<Self>) {
            self.received.push(msg);
        }
    }

    #[actix::test]
    async fn test_room_creation() {
        let actor = TestActor { received: vec![] }.start();
        let (room, _channels) = Room::new("test".to_string(), actor.recipient());

        // Should not panic
        assert_eq!(room.room_id(), "test");
    }
}
