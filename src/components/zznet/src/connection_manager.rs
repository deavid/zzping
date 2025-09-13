use crate::proto::messages::{ChannelId, DataMsg};
use crate::traits::AsyncReadWrite;
use anyhow::Result;
use log;
use rmp_serde;
use std::collections::HashMap;
use tokio::io::{ReadHalf, WriteHalf, split};
use tokio::sync::{mpsc, oneshot};

pub enum ConnectionCommand {
    SendFrame(Vec<u8>),
    RequestChannel {
        name: String,
        response_tx: oneshot::Sender<Result<Channel>>,
    },
    SendData(DataMsg),
}

/// Represents a single, logical communication channel. It provides an async method
/// for sending application-level data and contains the receiver for incoming data.
pub struct Channel {
    pub id: ChannelId,
    command_tx: mpsc::Sender<ConnectionCommand>,
    pub rx: mpsc::Receiver<DataMsg>,
}

impl Channel {
    /// Asynchronously sends a payload over the channel.
    pub async fn send(&self, payload: Vec<u8>) -> Result<()> {
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

/// Lightweight handle for interacting with an active connection. Its methods send commands to a background actor task that manages the actual connection state.
pub struct Connection {
    command_tx: mpsc::Sender<ConnectionCommand>,
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
}

impl Connection {
    /// Creates a new Connection instance with initialized channel management state.
    pub fn new(stream: Box<dyn AsyncReadWrite + Send + Unpin>) -> Self {
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
                                            // TODO: The ZzNet facade needs a way to receive newly opened channels from the server side.
                                            // For now, we log and drop the receiver to complete the handshake.
                                            log::info!("Channel '{}' opened with ID {}. Receiver is currently dropped.", name, id);
                                            drop(app_rx);
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
                                                if let Err(_) = response_tx.send(Ok(channel)) {
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
