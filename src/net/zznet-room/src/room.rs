use actix::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;

/// A cloneable typed sender for Room<T> messages
///
/// This wrapper provides typed message sending with automatic serialization,
/// and can be cloned and moved across async boundaries (unlike Room<T> itself).
#[derive(Clone)]
pub struct TypedSender<T>
where
    T: Serialize,
{
    outbound_tx: mpsc::Sender<Vec<u8>>,
    _phantom: std::marker::PhantomData<T>,
}

impl<T> TypedSender<T>
where
    T: Serialize,
{
    /// Send a typed message, automatically serializing it
    pub async fn send(&self, msg: T) -> Result<(), SendError> {
        let bytes = bincode::serde::encode_to_vec(&msg, bincode::config::standard())
            .map_err(|e| SendError::SerializationFailed(e.to_string()))?;

        self.outbound_tx
            .send(bytes)
            .await
            .map_err(|_| SendError::ChannelClosed)?;

        Ok(())
    }
}

/// A bidirectional typed communication channel between two components.
///
/// A `Room<T>` allows a local component to send and receive typed messages
/// of type `T` to/from a peer component. It handles serialization/deserialization
/// automatically and provides channels for connecting to a SessionManager.
///
#[derive(Debug)]
pub struct Room<T>
where
    T: Message<Result = ()> + Send + Clone + Serialize + for<'de> Deserialize<'de> + 'static,
{
    room_id: String,
    // Send serialized messages to peer (outbound - now Vec<u8>)
    outbound_tx: mpsc::Sender<Vec<u8>>,

    // Local component that handles received messages
    local_handler: Recipient<T>,

    // Background task that forwards inbound → handler. Always present.
    receiver_task: JoinHandle<()>,
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

#[derive(Debug, Error)]
/// Error returned when creating or registering a Room fails.
pub enum RoomError {
    #[error("Room {0} already registered")]
    /// Attempted to register a room that's already registered.
    AlreadyRegistered(String),
    #[error("SessionManager unavailable or locked")]
    /// Could not access SessionManager.
    SessionManagerUnavailable,
    #[error("Failed to register with SessionManager: {0}")]
    /// Registration operation failed.
    RegistrationFailed(String),
    #[error("Failed to spawn receiver task: {0}")]
    /// Could not spawn the background receiver task.
    ReceiverSpawnFailed(String),
}

/// Trait that SessionManager implements to allow Room auto-registration.
///
/// This trait defines the interface between Room<T> and SessionManager,
/// enabling automatic room registration without tight coupling.
pub trait RoomRegistry {
    /// Register a room handler with the SessionManager
    ///
    /// # Arguments
    ///
    /// * `room_id` - Unique identifier for this room
    /// * `inbound_tx` - Channel for sending messages to the room (SessionManager → Room)
    /// * `outbound_rx` - Channel for receiving messages from the room (Room → SessionManager)
    ///
    /// # Returns
    ///
    /// Ok(()) on success, or an error if registration fails (e.g., room already exists)
    fn register_room_handler(
        &mut self,
        room_id: String,
        inbound_tx: mpsc::Sender<Vec<u8>>,
        outbound_rx: mpsc::Receiver<Vec<u8>>,
    ) -> Result<(), Box<dyn std::error::Error>>;
}

impl<T> Room<T>
where
    T: Message<Result = ()> + Send + Clone + Serialize + for<'de> Deserialize<'de> + 'static,
{
    /// Create a new Room with the given room ID and local handler
    /// Returns the Room and channels for connecting to SessionManager
    ///
    /// **Legacy Method**: Use `new_with_session_manager()` for auto-registration in production.
    /// This method is kept for backward compatibility and testing scenarios.
    pub fn new(room_id: String, local_handler: Recipient<T>) -> (Self, RoomChannels) {
        let (outbound_tx, outbound_rx) = mpsc::channel(100);
        let (inbound_tx, mut inbound_rx) = mpsc::channel(100);

        // Spawn receiver task immediately and keep the JoinHandle on the struct.
        let handler_clone = local_handler.clone();
        let task = tokio::spawn(async move {
            while let Some(msg) = inbound_rx.recv().await {
                if let Err(e) = Self::handle_message(&handler_clone, msg).await {
                    tracing::error!("Error receiving message for room: {e:?}");
                }
            }
            tracing::debug!("Room receiver task stopped");
        });

        let room = Room {
            room_id,
            outbound_tx,
            local_handler,
            receiver_task: task,
        };

        let channels = RoomChannels {
            outbound_rx,
            inbound_tx,
        };

        (room, channels)
    }

    /// Create a new Room that auto-registers with SessionManager
    ///
    /// This is the recommended constructor for production use. The room will
    /// automatically register its channels with the SessionManager, eliminating
    /// the need for manual wiring.
    ///
    /// # Arguments
    ///
    /// * `room_id` - Unique identifier for this room
    /// * `local_handler` - Recipient that will receive deserialized messages
    /// * `session_manager` - Shared SessionManager instance for registration
    ///
    /// # Returns
    ///
    /// Returns the Room on success, or RoomError if registration fails.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let session_manager = Arc::new(Mutex::new(SessionManager::new(vec![])));
    /// let room = Room::new_with_session_manager(
    ///     "my-room".to_string(),
    ///     actor_addr.recipient(),
    ///     session_manager,
    /// )?;
    /// ```
    ///
    /// # Note
    ///
    /// This method requires SessionManager to implement `register_room_handler()`.
    /// The registration happens synchronously during construction (fail-fast).
    pub fn new_with_session_manager<SM>(
        room_id: String,
        local_handler: Recipient<T>,
        session_manager: Arc<Mutex<SM>>,
    ) -> Result<Self, RoomError>
    where
        SM: RoomRegistry + Send,
    {
        // Create channels
        let (outbound_tx, outbound_rx) = mpsc::channel(100);
        let (inbound_tx, inbound_rx) = mpsc::channel(100);

        // Register with SessionManager
        // Using try_lock() to avoid blocking, as this is called during actor construction
        let mut sm = session_manager
            .try_lock()
            .map_err(|_| RoomError::SessionManagerUnavailable)?;

        sm.register_room_handler(room_id.clone(), inbound_tx.clone(), outbound_rx)
            .map_err(|e| RoomError::RegistrationFailed(format!("{:?}", e)))?;

        // Drop the lock before spawning tasks
        drop(sm);

        // Create room
        let handler_clone = local_handler.clone();

        // Spawn receiver task immediately and keep the handle on the struct.
        let task = tokio::spawn(async move {
            let mut rx = inbound_rx;
            while let Some(msg) = rx.recv().await {
                if let Err(e) = Self::handle_message(&handler_clone, msg).await {
                    tracing::error!("Error receiving message for room: {e:?}");
                }
            }
            tracing::debug!("Room receiver task stopped");
        });

        let room = Room {
            room_id: room_id.clone(),
            outbound_tx,
            local_handler: local_handler.clone(),
            receiver_task: task,
        };

        tracing::info!(
            "Room '{}' created and registered with SessionManager",
            room_id
        );

        Ok(room)
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

    /// Get a cloneable typed sender that handles serialization automatically
    ///
    /// This returns a wrapper that can be cloned and moved into async contexts,
    /// allowing you to send typed messages without manual serialization.
    pub fn typed_sender(&self) -> TypedSender<T> {
        TypedSender {
            outbound_tx: self.outbound_tx.clone(),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Process one inbound message manually (for testing)
    /// Returns Ok(true) if message was processed
    /// Returns Ok(false) if no message available
    /// Returns Err if channel is closed
    #[deprecated(note = "This function has either to be removed or be called in spawn_receiver")]
    pub async fn process_one(&mut self) -> Result<bool, ProcessError> {
        // With the receiver spawned automatically, manual processing is not
        // supported. Keep the API but always indicate the receiver is spawned.
        Err(ProcessError::ReceiverAlreadySpawned)
    }

    /// Spawn a background task that automatically forwards inbound messages
    /// to the local handler. Can only be called once.
    pub fn spawn_receiver(&mut self) -> Result<(), SpawnError> {
        // Receiver is spawned during construction; external callers should not
        // attempt to spawn it again.
        Err(SpawnError::AlreadySpawned)
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

    #[derive(Message)]
    #[rtype(result = "Vec<TestMsg>")]
    struct GetReceivedMessages;

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

    impl Handler<GetReceivedMessages> for TestActor {
        type Result = Vec<TestMsg>;

        fn handle(&mut self, _msg: GetReceivedMessages, _ctx: &mut Context<Self>) -> Self::Result {
            self.received.clone()
        }
    }

    #[actix::test]
    async fn test_room_creation() {
        let actor = TestActor { received: vec![] }.start();
        let (room, _channels) = Room::<TestMsg>::new("test".to_string(), actor.recipient());

        // Should not panic
        assert_eq!(room.room_id(), "test");
    }

    // Mock SessionManager for testing
    struct MockSessionManager {
        registered_rooms: Vec<String>,
    }

    impl MockSessionManager {
        fn new() -> Self {
            MockSessionManager {
                registered_rooms: vec![],
            }
        }
    }

    impl RoomRegistry for MockSessionManager {
        fn register_room_handler(
            &mut self,
            room_id: String,
            _inbound_tx: mpsc::Sender<Vec<u8>>,
            _outbound_rx: mpsc::Receiver<Vec<u8>>,
        ) -> Result<(), Box<dyn std::error::Error>> {
            if self.registered_rooms.contains(&room_id) {
                return Err(format!("Room {} already registered", room_id).into());
            }
            self.registered_rooms.push(room_id);
            Ok(())
        }
    }

    #[actix::test]
    async fn test_room_with_session_manager() {
        let actor = TestActor { received: vec![] }.start();
        let session_manager = Arc::new(Mutex::new(MockSessionManager::new()));

        let room = Room::<TestMsg>::new_with_session_manager(
            "test-auto".to_string(),
            actor.recipient(),
            session_manager.clone(),
        );

        assert!(room.is_ok());
        let room = room.unwrap();
        assert_eq!(room.room_id(), "test-auto");

        // Verify registration occurred
        let sm = session_manager.lock().await;
        assert_eq!(sm.registered_rooms.len(), 1);
        assert_eq!(sm.registered_rooms[0], "test-auto");
    }

    #[actix::test]
    async fn test_room_registration_duplicate_fails() {
        let actor = TestActor { received: vec![] }.start();
        let session_manager = Arc::new(Mutex::new(MockSessionManager::new()));

        // First registration should succeed
        let room1 = Room::<TestMsg>::new_with_session_manager(
            "duplicate-room".to_string(),
            actor.recipient(),
            session_manager.clone(),
        );
        assert!(room1.is_ok());

        // Second registration should fail
        let actor2 = TestActor { received: vec![] }.start();
        let room2 = Room::<TestMsg>::new_with_session_manager(
            "duplicate-room".to_string(),
            actor2.recipient(),
            session_manager.clone(),
        );
        assert!(room2.is_err());

        match room2.unwrap_err() {
            RoomError::RegistrationFailed(msg) => {
                assert!(msg.contains("duplicate-room"));
            }
            _ => panic!("Expected RegistrationFailed error"),
        }
    }

    #[actix::test]
    async fn test_room_receiver_spawned_automatically() {
        let actor = TestActor { received: vec![] }.start();
        let session_manager = Arc::new(Mutex::new(MockSessionManager::new()));

        let room = Room::<TestMsg>::new_with_session_manager(
            "auto-spawn".to_string(),
            actor.recipient(),
            session_manager,
        )
        .unwrap();

        // Verify receiver task was spawned (it should be running)
        assert!(!room.receiver_task.is_finished());
    }

    // --- Serialization/Deserialization Roundtrip Tests ---

    #[actix::test]
    async fn test_serialization_roundtrip_simple() {
        // Test that a message can be serialized and deserialized correctly
        let msg = TestMsg { value: 42 };

        // Serialize
        let bytes = bincode::serde::encode_to_vec(&msg, bincode::config::standard())
            .expect("serialization failed");

        // Deserialize
        let (decoded, _): (TestMsg, _) =
            bincode::serde::decode_from_slice(&bytes, bincode::config::standard())
                .expect("deserialization failed");

        assert_eq!(decoded, msg);
    }

    #[actix::test]
    async fn test_send_and_receive_message() {
        let actor = TestActor { received: vec![] }.start();
        let (room, channels) = Room::<TestMsg>::new("roundtrip".to_string(), actor.recipient());

        // Create a sender task that sends a message
        let msg_to_send = TestMsg { value: 123 };
        let send_task = tokio::spawn(async move {
            room.send(msg_to_send.clone()).await.expect("send failed");
        });

        // The message should be received on the outbound channel
        let mut rx = channels.outbound_rx;
        let received_bytes = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("timeout")
            .expect("channel closed");

        // Verify we can deserialize it back
        let (decoded, _): (TestMsg, _) =
            bincode::serde::decode_from_slice(&received_bytes, bincode::config::standard())
                .expect("deserialization failed");

        assert_eq!(decoded.value, 123);
        send_task.await.expect("send task failed");
    }

    #[actix::test]
    async fn test_inbound_message_handling_with_deserialization() {
        let actor = TestActor { received: vec![] }.start();
        let actor_addr = actor.clone();

        let (_room, channels) = Room::<TestMsg>::new("inbound".to_string(), actor_addr.recipient());

        // Create a message, serialize it, and send it through the inbound channel
        let test_msg = TestMsg { value: 999 };
        let serialized =
            bincode::serde::encode_to_vec(&test_msg, bincode::config::standard()).unwrap();

        // Send through inbound channel
        channels
            .inbound_tx
            .send(serialized)
            .await
            .expect("inbound send failed");

        // Give the receiver task time to process
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Verify the actor received the deserialized message
        let received = actor.send(GetReceivedMessages).await.unwrap();
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].value, 999);
    }

    #[actix::test]
    async fn test_multiple_message_roundtrip() {
        let actor = TestActor { received: vec![] }.start();
        let actor_addr = actor.clone();

        let (_room, channels) = Room::<TestMsg>::new("multi".to_string(), actor_addr.recipient());

        // Send multiple messages
        let messages = [
            TestMsg { value: 1 },
            TestMsg { value: 2 },
            TestMsg { value: 3 },
        ];

        for msg in messages.iter() {
            let serialized = bincode::serde::encode_to_vec(msg, bincode::config::standard())
                .expect("serialization failed");
            channels
                .inbound_tx
                .send(serialized)
                .await
                .expect("send failed");
        }

        // Give receiver time to process all messages
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Verify all messages were received and deserialized
        let received = actor.send(GetReceivedMessages).await.unwrap();
        assert_eq!(received.len(), 3);
        assert_eq!(received[0].value, 1);
        assert_eq!(received[1].value, 2);
        assert_eq!(received[2].value, 3);
    }

    #[actix::test]
    async fn test_deserialization_error_handling() {
        let actor = TestActor { received: vec![] }.start();
        let actor_addr = actor.clone();

        let (_room, channels) =
            Room::<TestMsg>::new("error-test".to_string(), actor_addr.recipient());

        // Send invalid bytes that can't be deserialized
        let invalid_bytes = vec![0xFF, 0xFE, 0xFD, 0xFC];
        let result = channels.inbound_tx.send(invalid_bytes).await;

        // Should succeed in sending (error happens during deserialization in receiver task)
        assert!(result.is_ok());

        // Give time for receiver to process and fail
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // The actor should have received no valid messages
        let received = actor.send(GetReceivedMessages).await.unwrap();
        assert_eq!(received.len(), 0);
    }
}
