use actix::prelude::*;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// A bidirectional typed communication channel between two components.
///
/// A `Room<T>` allows a local component to send and receive typed messages
/// of type `T` to/from a peer component without any knowledge of serialization
/// or transport.
///
/// # Example
///
/// ```rust
/// use zznet_room::room::Room;
/// use actix::prelude::*;
///
/// #[derive(Message, Clone)]
/// #[rtype(result = "()")]
/// struct MyMessage { value: i32 }
///
/// struct MyActor;
/// impl Actor for MyActor { type Context = Context<Self>; }
/// impl Handler<MyMessage> for MyActor {
///     type Result = ();
///     fn handle(&mut self, msg: MyMessage, _: &mut Context<Self>) {}
/// }
///
/// // In an Actix runtime:
/// // let actor = MyActor.start();
/// // let (room, channels) = Room::new(actor.recipient());
/// ```
pub struct Room<T>
where
    T: Message<Result = ()> + Send + Clone + 'static,
{
    // Send messages to peer (outbound)
    outbound_tx: mpsc::Sender<T>,

    // Receive messages from peer (inbound)
    inbound_rx: Option<mpsc::Receiver<T>>,

    // Local component that handles received messages
    local_handler: Recipient<T>,

    // Background task that forwards inbound → handler
    receiver_task: Option<JoinHandle<()>>,
}

/// Channels returned when creating a Room, used for wiring with connect_rooms()
pub struct RoomChannels<T> {
    /// Receiver for outbound messages (to be forwarded to peer)
    pub outbound_rx: mpsc::Receiver<T>,
    /// Sender for inbound messages (from peer)
    pub inbound_tx: mpsc::Sender<T>,
}

#[derive(Debug, Error)]
pub enum ProcessError {
    #[error("Receiver already spawned in background")]
    ReceiverAlreadySpawned,
    #[error("Handler failed to process message")]
    HandlerFailed,
    #[error("Channel closed")]
    ChannelClosed,
}

#[derive(Debug, Error)]
pub enum SpawnError {
    #[error("Receiver already spawned")]
    AlreadySpawned,
}

impl<T> Room<T>
where
    T: Message<Result = ()> + Send + Clone + 'static,
{
    /// Create a new Room with the given local handler
    /// Returns the Room and the channels for wiring
    pub fn new(local_handler: Recipient<T>) -> (Self, RoomChannels<T>) {
        let (outbound_tx, outbound_rx) = mpsc::channel(100);
        let (inbound_tx, inbound_rx) = mpsc::channel(100);

        let room = Room {
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
    pub async fn send(&self, msg: T) -> Result<(), mpsc::error::SendError<T>> {
        self.outbound_tx.send(msg).await
    }

    /// Process one inbound message manually (for testing)
    /// Returns Ok(true) if message was processed
    /// Returns Ok(false) if no message available
    /// Returns Err if channel is closed
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
                // Ignore errors (handler might be stopped)
                let _ = Self::handle_message(&handler, msg).await;
            }
            tracing::debug!("Room receiver task stopped");
        });

        self.receiver_task = Some(task);
        Ok(())
    }

    /// Internal helper to handle a single message by forwarding to the handler
    async fn handle_message(handler: &Recipient<T>, msg: T) -> Result<(), ProcessError> {
        handler
            .send(msg)
            .await
            .map_err(|_| ProcessError::HandlerFailed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Message, Clone, Debug, PartialEq)]
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
    async fn test_room_send() {
        let actor = TestActor { received: vec![] }.start();
        let (room, _channels) = Room::new(actor.recipient());

        // Should not panic
        room.send(TestMsg { value: 42 }).await.unwrap();
    }

    #[actix::test]
    async fn test_room_process_one() {
        let actor = TestActor { received: vec![] }.start();
        let (mut room, channels) = Room::new(actor.recipient());

        // Send a message to inbound
        channels
            .inbound_tx
            .send(TestMsg { value: 42 })
            .await
            .unwrap();

        // Process it manually
        let processed = room.process_one().await.unwrap();
        assert!(processed);

        // No more messages
        let processed = room.process_one().await.unwrap();
        assert!(!processed);
    }
}
