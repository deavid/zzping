use crate::traits::AsyncReadWrite;
use log;
use rmp_serde;
use tokio::io::{ReadHalf, WriteHalf, split};
use tokio::sync::mpsc;
use tokio::task;

/// Represents a single, active client-server connection and is responsible for managing
/// the multiplexing of all logical channels over this one connection.
pub struct Connection {
    stream: Box<dyn AsyncReadWrite + Send + Unpin>,
    tx: mpsc::Sender<Vec<u8>>,
    rx: mpsc::Receiver<Vec<u8>>,
}

impl Connection {
    pub fn new(stream: Box<dyn AsyncReadWrite + Send + Unpin>) -> Self {
        let (tx, rx) = mpsc::channel(32);
        Self { stream, tx, rx }
    }

    pub fn sender(&self) -> mpsc::Sender<Vec<u8>> {
        self.tx.clone()
    }

    /// Starts the connection's processing loops and will run until the underlying
    /// connection is closed or an error occurs.
    pub async fn run(self) {
        let Connection { stream, tx: _, rx } = self;
        let (reader, writer) = split(stream);

        let read_task = task::spawn(async move {
            Self::read_loop(reader).await;
        });

        let write_task = task::spawn(async move {
            Self::write_loop(writer, rx).await;
        });

        let _ = tokio::try_join!(read_task, write_task);
    }

    async fn read_loop(mut reader: ReadHalf<Box<dyn AsyncReadWrite + Send + Unpin>>) {
        loop {
            match crate::proto::frame::read_frame(&mut reader).await {
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
                                    log::debug!("Request to open channel '{}'", name)
                                }
                                crate::proto::messages::ControlMsg::ChannelOpened { name, id } => {
                                    log::debug!("Channel '{}' opened with ID {}", name, id)
                                }
                                crate::proto::messages::ControlMsg::CloseChannel { id } => {
                                    log::debug!("Close channel {}", id)
                                }
                            },
                            crate::proto::messages::Frame::Data(data) => {
                                log::debug!("Received data for channel ID {}", data.channel_id)
                            }
                        },
                        Err(e) => {
                            log::warn!(
                                "Fatal protocol error: failed to deserialize frame: {}. Dropping connection.",
                                e
                            );
                            break; // Terminate the loop and the connection
                        }
                    }
                }
                Err(e) => {
                    log::debug!("Read error: {:?}", e);
                    break;
                }
            }
        }
    }

    async fn write_loop(
        mut writer: WriteHalf<Box<dyn AsyncReadWrite + Send + Unpin>>,
        mut rx: mpsc::Receiver<Vec<u8>>,
    ) {
        while let Some(frame) = rx.recv().await {
            if let Err(e) = crate::proto::frame::write_frame(&mut writer, &frame).await {
                log::debug!("Write error: {:?}", e);
                break;
            }
        }
    }
}
