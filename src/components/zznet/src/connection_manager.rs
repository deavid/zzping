use crate::proto::messages::{ChannelId, DataMsg};
use crate::traits::AsyncReadWrite;
use anyhow::Result;
use async_trait::async_trait;
use log;
use rmp_serde;
use std::collections::HashMap;
use tokio::io::{ReadHalf, WriteHalf, split};
use tokio::sync::{mpsc, oneshot};
use zznet_api::ZzChannel;

pub enum ConnectionCommand {
    SendFrame(Vec<u8>),
    RequestChannel {
        name: String,
        response_tx: oneshot::Sender<Result<Channel>>,
    },
    SendData(DataMsg),
}

/// Represents events that the ConnectionActor sends upward to its manager.
/// This allows the facade to receive notifications about channel openings.
pub enum ConnectionEvent {
    ChannelOpened {
        name: String,
        id: ChannelId,
        receiver: mpsc::Receiver<DataMsg>,
    },
}

/// Represents a single, logical communication channel. It provides an async method
/// for sending application-level data and contains the receiver for incoming data.
pub struct Channel {
    pub id: ChannelId,
    pub command_tx: mpsc::Sender<ConnectionCommand>,
    pub rx: mpsc::Receiver<DataMsg>,
}

impl Channel {
    /// Asynchronously sends a payload over the channel.
    pub async fn send_actor_command(&self, payload: Vec<u8>) -> Result<()> {
        let data_msg = DataMsg {
            channel_id: self.id,
            payload,
        };
        self.command_tx
            .send(ConnectionCommand::SendData(data_msg))
            .await?;
        Ok(())
    }
}

#[async_trait]
impl ZzChannel for Channel {
    async fn send(&self, payload: Vec<u8>) -> Result<()> {
        self.send_actor_command(payload).await
    }

    async fn recv(&mut self) -> Result<Option<Vec<u8>>> {
        match self.rx.recv().await {
            Some(data_msg) => Ok(Some(data_msg.payload)),
            None => Ok(None),
        }
    }
}

/// Lightweight handle for interacting with an active connection. Its methods send commands to a background actor task that manages the actual connection state.
#[derive(Debug, Clone)]
pub struct Connection {
    command_tx: mpsc::Sender<ConnectionCommand>,
}

impl Connection {
    /// Creates a new Connection instance with initialized channel management state.
    /// The event_tx is used to send upward events like channel openings to the facade.
    pub fn new(
        stream: Box<dyn AsyncReadWrite + Send + Unpin>,
        event_tx: mpsc::Sender<ConnectionEvent>,
    ) -> Self {
        let (command_tx, command_rx) = mpsc::channel(32);
        let (reader, writer) = split(stream);
        let actor = ConnectionActor {
            reader,
            writer,
            channels_by_id: HashMap::new(),
            channels_by_name: HashMap::new(),
            next_channel_id: 1,
            command_rx,
            command_tx: command_tx.clone(),
            pending_requests: HashMap::new(),
            event_tx,
        };
        tokio::spawn(actor.run());
        Self { command_tx }
    }

    pub async fn send_frame(
        &self,
        frame: Vec<u8>,
    ) -> Result<(), mpsc::error::SendError<ConnectionCommand>> {
        self.command_tx
            .send(ConnectionCommand::SendFrame(frame))
            .await
    }

    /// Requests a new channel with the given name from the server.
    // TODO: Add a timeout to this function. If the remote peer never responds, the oneshot::Sender
    // in `pending_requests` will be held forever, and the application task will remain awaiting a
    // response indefinitely.
    pub async fn request_channel(&self, name: String) -> Result<Channel> {
        let (response_tx, response_rx) = oneshot::channel();
        self.command_tx
            .send(ConnectionCommand::RequestChannel { name, response_tx })
            .await?;
        response_rx.await?
    }

    pub fn command_sender(&self) -> mpsc::Sender<ConnectionCommand> {
        self.command_tx.clone()
    }
}

/// Private, internal state machine for a connection. It runs in its own task and is not meant to be accessed directly.
struct ConnectionActor {
    reader: ReadHalf<Box<dyn AsyncReadWrite + Send + Unpin>>,
    writer: WriteHalf<Box<dyn AsyncReadWrite + Send + Unpin>>,
    channels_by_id: HashMap<ChannelId, mpsc::Sender<DataMsg>>,
    // FIXME: On the client side, this map is populated but never used.
    // When we implement channel closing, we'll need to decide if we want to close by name or by ID,
    // which will determine if this map is needed on the client. For now, it's harmless.
    channels_by_name: HashMap<String, ChannelId>,
    next_channel_id: ChannelId,
    command_rx: mpsc::Receiver<ConnectionCommand>,
    command_tx: mpsc::Sender<ConnectionCommand>,
    pending_requests: HashMap<String, oneshot::Sender<Result<Channel>>>,
    event_tx: mpsc::Sender<ConnectionEvent>,
}

impl ConnectionActor {
    async fn run(mut self) {
        loop {
            tokio::select! {
                Some(command) = self.command_rx.recv() => {
                    match command {
                        ConnectionCommand::SendFrame(frame) => {
                            if let Err(e) = crate::proto::frame::write_frame(&mut self.writer, &frame).await {
                                log::debug!("Write error: {}. Terminating connection.", e);
                                break;
                            }
                        }
                        ConnectionCommand::RequestChannel { name, response_tx } => {
                            self.pending_requests.insert(name.clone(), response_tx);
                            let request = crate::proto::messages::Frame::Control(
                                crate::proto::messages::ControlMsg::RequestChannel { name },
                            );
                            match rmp_serde::encode::to_vec(&request) {
                                Ok(serialized) => {
                                    if let Err(e) = crate::proto::frame::write_frame(&mut self.writer, &serialized).await {
                                        log::warn!("Failed to send channel request: {}", e);
                                    }
                                }
                                Err(e) => log::warn!(
                                    "Failed to serialize channel request: {}",
                                    e
                                ),
                            }
                        }
                        ConnectionCommand::SendData(data_msg) => {
                            let frame = crate::proto::messages::Frame::Data(data_msg);
                            match rmp_serde::encode::to_vec(&frame) {
                                Ok(serialized) => {
                                    if let Err(e) = crate::proto::frame::write_frame(&mut self.writer, &serialized).await {
                                        log::warn!("Failed to send data: {}", e);
                                    }
                                }
                                Err(e) => log::warn!(
                                    "Failed to serialize data frame: {}",
                                    e
                                ),
                            }
                        }
                    }
                }
                result = crate::proto::frame::read_frame(&mut self.reader) => {
                    match result {
                        Ok(frame_bytes) => {
                            match rmp_serde::decode::from_slice::<crate::proto::messages::Frame>(
                                &frame_bytes,
                            ) {
                                Ok(frame) => match frame {
                                    crate::proto::messages::Frame::Control(ctrl) => match ctrl {
                                        crate::proto::messages::ControlMsg::Hello(_) => {
                                            log::debug!("Hello message")
                                        }
                                        crate::proto::messages::ControlMsg::RequestChannel { name } => {
                                            let id = {
                                                let current = self.next_channel_id;
                                                self.next_channel_id += 1;
                                                current
                                            };
                                            let (app_tx, app_rx) = mpsc::channel(32);
                                            self.channels_by_id.insert(id, app_tx);
                                            self.channels_by_name.insert(name.clone(), id);
                                            // Send the channel opened event to the facade instead of dropping the receiver.
                                            let event = ConnectionEvent::ChannelOpened {
                                                name: name.clone(),
                                                id,
                                                receiver: app_rx,
                                            };
                                            if self.event_tx.send(event).await.is_err() {
                                                log::warn!("Failed to send ChannelOpened event to manager; receiver dropped.");
                                            }
                                            let response = crate::proto::messages::Frame::Control(
                                                crate::proto::messages::ControlMsg::ChannelOpened {
                                                    name,
                                                    id,
                                                },
                                            );
                                            match rmp_serde::encode::to_vec(&response) {
                                                Ok(serialized) => {
                                                    if let Err(e) = crate::proto::frame::write_frame(&mut self.writer, &serialized).await {
                                                        log::warn!("Failed to send channel opened: {}", e);
                                                    }
                                                }
                                                Err(e) => log::warn!(
                                                    "Failed to serialize channel opened response: {}",
                                                    e
                                                ),
                                            }
                                        }
                                        crate::proto::messages::ControlMsg::ChannelOpened { name, id } => {
                                            if let Some(response_tx) = self.pending_requests.remove(&name) {
                                                let (app_tx, app_rx) = mpsc::channel(32);
                                                self.channels_by_id.insert(id, app_tx);
                                                self.channels_by_name.insert(name.clone(), id);
                                                let channel = Channel {
                                                    id,
                                                    command_tx: self.command_tx.clone(),
                                                    rx: app_rx,
                                                };
                                                if response_tx.send(Ok(channel)).is_err() {
                                                    log::warn!("Failed to send channel to application");
                                                }
                                            } else {
                                                log::warn!("Received ChannelOpened for unknown request: {}", name);
                                            }
                                        }
                                        crate::proto::messages::ControlMsg::CloseChannel { id } => {
                                            log::debug!("Close channel {}", id)
                                        }
                                    },
                                    crate::proto::messages::Frame::Data(data) => {
                                        let channel_id = data.channel_id;
                                        let sender = self.channels_by_id.get(&channel_id).cloned();
                                        if let Some(sender) = sender {
                                            if let Err(e) = sender.send(data).await {
                                                log::warn!(
                                                    "Failed to send data to channel {}: {}",
                                                    channel_id,
                                                    e
                                                );
                                            }
                                        } else {
                                            log::warn!(
                                                "Received data for unknown channel ID {}",
                                                channel_id
                                            );
                                        }
                                    }
                                },
                                Err(e) => {
                                    log::warn!(
                                        "Fatal protocol error: failed to deserialize frame: {}. Dropping connection.",
                                        e
                                    );
                                    break;
                                }
                            }
                        }
                        Err(e) => {
                            log::debug!("Read error: {}. Terminating connection.", e);
                            break;
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::messages::{ControlMsg, Frame};
    use log::info;
    use ntest::timeout;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
    use tokio::sync::mpsc;

    // A mock stream that allows a test to deterministically control I/O
    struct MockStream {
        // The test sends bytes here for the actor to "read"
        rx_for_stream: mpsc::Receiver<Vec<u8>>,
        // The actor sends bytes here for the test to "assert on"
        tx_for_test: mpsc::Sender<Vec<u8>>,
        // Internal buffer for partial reads
        buffer: Vec<u8>,
    }

    impl AsyncRead for MockStream {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            // If we have leftover data, use it.
            if !self.buffer.is_empty() {
                let len = std::cmp::min(buf.remaining(), self.buffer.len());
                buf.put_slice(&self.buffer[..len]);
                self.buffer.drain(..len);
                return Poll::Ready(Ok(()));
            }

            // Otherwise, try to get more data from the test.
            match self.rx_for_stream.poll_recv(cx) {
                Poll::Ready(Some(data)) => {
                    // We got data. Put it in the buffer and then fill buf from it.
                    self.buffer.extend_from_slice(&data);
                    let len = std::cmp::min(buf.remaining(), self.buffer.len());
                    buf.put_slice(&self.buffer[..len]);
                    self.buffer.drain(..len);
                    Poll::Ready(Ok(()))
                }
                Poll::Ready(None) => Poll::Ready(Ok(())), // Stream closed
                Poll::Pending => Poll::Pending,
            }
        }
    }

    impl AsyncWrite for MockStream {
        fn poll_write(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            // When the actor writes, send the data to the test
            match self.tx_for_test.try_send(buf.to_vec()) {
                Ok(_) => Poll::Ready(Ok(buf.len())),
                Err(_) => Poll::Ready(Err(std::io::Error::new(
                    std::io::ErrorKind::WriteZero,
                    "Test channel closed",
                ))),
            }
        }
        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }
        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    #[timeout(100)]
    async fn test_request_channel_sends_frame() {
        let _ = env_logger::builder().is_test(true).try_init();
        info!("SETUP: test_request_channel_sends_frame");

        // SETUP: Create the mpsc channels for our mock stream
        let (_tx_to_stream, rx_for_stream) = mpsc::channel(32);
        let (tx_for_test, mut rx_from_stream) = mpsc::channel(32);

        let mock_stream = MockStream {
            rx_for_stream,
            tx_for_test,
            buffer: Vec::new(),
        };

        // Create the connection with the mock stream
        let (event_tx, _event_rx) = mpsc::channel(32);
        let connection = Connection::new(Box::new(mock_stream), event_tx);

        info!("ACT: Request a channel. This sends a command to the actor.");
        // We don't await this future yet. It will complete when the actor responds.
        // Here, we just want to trigger the actor to send a frame.
        let command_tx = connection.command_sender();
        let (response_tx, _response_rx) = oneshot::channel();
        command_tx
            .send(ConnectionCommand::RequestChannel {
                name: "test".to_string(),
                response_tx,
            })
            .await
            .unwrap();

        info!("ASSERT: Block and wait to receive the frame the actor wrote to the stream.");
        // This is the synchronization point. No sleep needed.
        // The actor's write_frame performs two writes, one for length and one for data.
        let written_len = rx_from_stream
            .recv()
            .await
            .expect("Actor did not write length to stream");
        let written_data = rx_from_stream
            .recv()
            .await
            .expect("Actor did not write data to stream");

        // Verify the frame is correct
        let expected_frame = Frame::Control(ControlMsg::RequestChannel {
            name: "test".to_string(),
        });
        let expected_bytes = rmp_serde::to_vec(&expected_frame).unwrap();
        assert_eq!(written_len, (expected_bytes.len() as u32).to_be_bytes());
        assert_eq!(written_data, expected_bytes);
    }

    #[tokio::test]
    #[timeout(200)]
    async fn test_channel_opened_completes_request() {
        let _ = env_logger::builder().is_test(true).try_init();
        info!("SETUP: test_channel_opened_completes_request");

        let (tx_to_stream, rx_for_stream) = mpsc::channel(32);
        let (tx_for_test, mut rx_from_stream) = mpsc::channel(32);

        let mock_stream = MockStream {
            rx_for_stream,
            tx_for_test,
            buffer: Vec::new(),
        };
        let (event_tx, _event_rx) = mpsc::channel(32);
        let connection = Connection::new(Box::new(mock_stream), event_tx);
        let command_tx = connection.command_sender();
        let (response_tx, response_rx) = oneshot::channel(); // Manual oneshot
        let channel_name = "test-channel".to_string();
        let channel_id = 42;

        info!("ACT 1: Send RequestChannel command");
        command_tx
            .send(ConnectionCommand::RequestChannel {
                name: channel_name.clone(),
                response_tx,
            })
            .await
            .unwrap();

        info!("SYNC 1: Wait for actor to send frame");
        let _ = rx_from_stream.recv().await; // Drain length
        let _ = rx_from_stream.recv().await; // Drain data

        info!("ACT 2: Send ChannelOpened frame back");
        let response_frame = Frame::Control(ControlMsg::ChannelOpened {
            name: channel_name.clone(),
            id: channel_id,
        });
        let response_bytes = rmp_serde::to_vec(&response_frame).unwrap();
        let len_bytes = (response_bytes.len() as u32).to_be_bytes();
        let mut framed_response = Vec::with_capacity(4 + response_bytes.len());
        framed_response.extend_from_slice(&len_bytes);
        framed_response.extend_from_slice(&response_bytes);
        tx_to_stream.send(framed_response).await.unwrap();

        info!("ASSERT: Await the oneshot receiver");
        let channel = tokio::time::timeout(std::time::Duration::from_millis(150), response_rx)
            .await
            .expect("Test timed out")
            .unwrap() // Unwrap result from oneshot
            .unwrap(); // Unwrap result from actor

        assert_eq!(channel.id, channel_id);
    }

    #[tokio::test]
    #[timeout(200)]
    async fn test_data_frame_forwards_to_channel() {
        let _ = env_logger::builder().is_test(true).try_init();
        info!("SETUP: test_data_frame_forwards_to_channel");

        let (tx_to_stream, rx_for_stream) = mpsc::channel(32);
        let (tx_for_test, mut rx_from_stream) = mpsc::channel(32);
        let mock_stream = MockStream {
            rx_for_stream,
            tx_for_test,
            buffer: Vec::new(),
        };
        let (event_tx, _event_rx) = mpsc::channel(32);
        let connection = Connection::new(Box::new(mock_stream), event_tx);
        let command_tx = connection.command_sender();
        let (response_tx, response_rx) = oneshot::channel();
        let channel_name = "test-channel".to_string();
        let channel_id = 42;

        info!("ACT 1: Establish a channel");
        command_tx
            .send(ConnectionCommand::RequestChannel {
                name: channel_name.clone(),
                response_tx,
            })
            .await
            .unwrap();
        let _ = rx_from_stream.recv().await; // Drain length
        let _ = rx_from_stream.recv().await; // Drain data
        let open_frame = Frame::Control(ControlMsg::ChannelOpened {
            name: channel_name.clone(),
            id: channel_id,
        });
        let open_bytes = rmp_serde::to_vec(&open_frame).unwrap();
        let len_bytes = (open_bytes.len() as u32).to_be_bytes();
        let mut framed_open = Vec::with_capacity(4 + open_bytes.len());
        framed_open.extend_from_slice(&len_bytes);
        framed_open.extend_from_slice(&open_bytes);
        tx_to_stream.send(framed_open).await.unwrap();
        let mut channel = tokio::time::timeout(std::time::Duration::from_millis(150), response_rx)
            .await
            .expect("Channel open timed out")
            .unwrap()
            .unwrap();

        info!("ACT 2: Send a Data frame to the actor");
        let payload = vec![1, 2, 3, 4];
        let data_frame = Frame::Data(crate::proto::messages::DataMsg {
            channel_id,
            payload: payload.clone(),
        });
        let data_bytes = rmp_serde::to_vec(&data_frame).unwrap();
        let len_bytes = (data_bytes.len() as u32).to_be_bytes();
        let mut framed_data = Vec::with_capacity(4 + data_bytes.len());
        framed_data.extend_from_slice(&len_bytes);
        framed_data.extend_from_slice(&data_bytes);
        tx_to_stream.send(framed_data).await.unwrap();

        info!("ASSERT: The channel's receiver gets the data");
        let received_data =
            tokio::time::timeout(std::time::Duration::from_millis(50), channel.rx.recv())
                .await
                .expect("Data receive timed out")
                .unwrap();
        assert_eq!(received_data.payload, payload);
    }

    #[tokio::test]
    #[timeout(100)]
    async fn test_data_for_unknown_channel_is_ignored() {
        let _ = env_logger::builder().is_test(true).try_init();
        let (tx_to_stream, rx_for_stream) = mpsc::channel(32);
        let (tx_for_test, _rx_from_stream) = mpsc::channel(32);
        let mock_stream = MockStream {
            rx_for_stream,
            tx_for_test,
            buffer: Vec::new(),
        };
        let (event_tx, _event_rx) = mpsc::channel(32);
        let connection = Connection::new(Box::new(mock_stream), event_tx);

        info!("ACT: Send a Data frame for a channel ID that doesn't exist");
        let data_frame = Frame::Data(crate::proto::messages::DataMsg {
            channel_id: 999, // Unknown ID
            payload: vec![5, 6, 7, 8],
        });
        let data_bytes = rmp_serde::to_vec(&data_frame).unwrap();
        let len_bytes = (data_bytes.len() as u32).to_be_bytes();
        let mut framed_data = Vec::with_capacity(4 + data_bytes.len());
        framed_data.extend_from_slice(&len_bytes);
        framed_data.extend_from_slice(&data_bytes);
        tx_to_stream.send(framed_data).await.unwrap();

        info!("ASSERT: The actor does not crash and its command channel remains open");
        // We can't easily inspect logs, but we can check the actor is still alive
        // by verifying its command channel hasn't been closed.
        assert!(!connection.command_tx.is_closed());
    }

    #[tokio::test]
    #[timeout(100)]
    async fn test_unsolicited_channel_opened_is_ignored() {
        let _ = env_logger::builder().is_test(true).try_init();
        let (tx_to_stream, rx_for_stream) = mpsc::channel(32);
        let (tx_for_test, _rx_from_stream) = mpsc::channel(32);
        let mock_stream = MockStream {
            rx_for_stream,
            tx_for_test,
            buffer: Vec::new(),
        };
        let (event_tx, _event_rx) = mpsc::channel(32);
        let connection = Connection::new(Box::new(mock_stream), event_tx);

        info!("ACT: Send a ChannelOpened frame for a request that was never made");
        let frame = Frame::Control(ControlMsg::ChannelOpened {
            name: "unsolicited-channel".to_string(),
            id: 123,
        });
        let bytes = rmp_serde::to_vec(&frame).unwrap();
        let len_bytes = (bytes.len() as u32).to_be_bytes();
        let mut framed = Vec::new();
        framed.extend_from_slice(&len_bytes);
        framed.extend_from_slice(&bytes);
        tx_to_stream.send(framed).await.unwrap();

        info!("ASSERT: The actor does not crash and its command channel remains open");
        assert!(!connection.command_tx.is_closed());
    }

    #[tokio::test]
    #[timeout(100)]
    async fn test_actor_terminates_on_read_error() {
        let _ = env_logger::builder().is_test(true).try_init();
        let (tx_to_stream, rx_for_stream) = mpsc::channel(32);
        let (tx_for_test, _rx_from_stream) = mpsc::channel(32);
        let mock_stream = MockStream {
            rx_for_stream,
            tx_for_test,
            buffer: Vec::new(),
        };
        let (event_tx, _event_rx) = mpsc::channel(32);
        let connection = Connection::new(Box::new(mock_stream), event_tx);

        info!("ACT: Close the stream from the test side");
        drop(tx_to_stream);

        info!("ASSERT: The actor's command channel is eventually closed");
        // The actor should terminate, which will drop its command_tx clone.
        // Once all clones are dropped (including the one in `connection`),
        // the channel will be closed.
        connection.command_tx.closed().await;
    }

    #[tokio::test]
    #[timeout(100)]
    async fn test_dropped_oneshot_receiver_does_not_panic() {
        let _ = env_logger::builder().is_test(true).try_init();
        let (tx_to_stream, rx_for_stream) = mpsc::channel(32);
        let (tx_for_test, mut rx_from_stream) = mpsc::channel(32);
        let mock_stream = MockStream {
            rx_for_stream,
            tx_for_test,
            buffer: Vec::new(),
        };
        let (event_tx, _event_rx) = mpsc::channel(32);
        let connection = Connection::new(Box::new(mock_stream), event_tx);
        let command_tx = connection.command_sender();
        let (response_tx, response_rx) = oneshot::channel();

        info!("ACT 1: Send request and drop receiver");
        command_tx
            .send(ConnectionCommand::RequestChannel {
                name: "dropped-request".to_string(),
                response_tx,
            })
            .await
            .unwrap();
        drop(response_rx); // This is the key part of the test

        info!("SYNC: Drain the request frame from the actor");
        let _ = rx_from_stream.recv().await;
        let _ = rx_from_stream.recv().await;

        info!("ACT 2: Simulate the peer opening the channel anyway");
        let frame = Frame::Control(ControlMsg::ChannelOpened {
            name: "dropped-request".to_string(),
            id: 777,
        });
        let bytes = rmp_serde::to_vec(&frame).unwrap();
        let len_bytes = (bytes.len() as u32).to_be_bytes();
        let mut framed = Vec::new();
        framed.extend_from_slice(&len_bytes);
        framed.extend_from_slice(&bytes);
        tx_to_stream.send(framed).await.unwrap();

        info!("ASSERT: The actor does not crash");
        // Give the actor a moment to process the ChannelOpened and the failed oneshot send
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        assert!(!connection.command_tx.is_closed());
    }
}
