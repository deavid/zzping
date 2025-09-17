use crate::proto::{ControlMsg, Frame};
use crate::traits::AsyncReadWrite;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::io::{split, AsyncWriteExt, WriteHalf};
use tokio::sync::{mpsc, Mutex};
use zznet_api::ZzRoom;

type ChannelId = u16;

#[derive(Debug)]
pub enum ConnectionEvent {
    ChannelOpened {
        name: String,
        id: ChannelId,
        receiver: mpsc::Receiver<Vec<u8>>,
    },
}

#[derive(Debug, Clone)]
pub struct Channel {
    pub id: ChannelId,
    command_tx: mpsc::Sender<Vec<u8>>,
    rx: Arc<Mutex<mpsc::Receiver<Vec<u8>>>>,
}

impl Channel {
    pub fn new(
        id: ChannelId,
        command_tx: mpsc::Sender<Vec<u8>>,
        rx: mpsc::Receiver<Vec<u8>>,
    ) -> Self {
        Self {
            id,
            command_tx,
            rx: Arc::new(Mutex::new(rx)),
        }
    }
}

#[async_trait]
impl ZzRoom for Channel {
    async fn send(&self, payload: Vec<u8>) -> Result<()> {
        self.command_tx.send(payload).await?;
        Ok(())
    }

    async fn recv(&mut self) -> Result<Option<Vec<u8>>> {
        let mut guard = self.rx.lock().await;
        Ok(guard.recv().await)
    }
}

#[derive(Debug, Clone)]
pub struct Connection {
    writer_tx: mpsc::Sender<Vec<u8>>,
}

impl Connection {
    pub fn new(
        stream: Box<dyn AsyncReadWrite + Send + Unpin>,
        _event_tx: mpsc::Sender<ConnectionEvent>,
    ) -> Self {
        let (_reader, writer) = split(stream);
        let (writer_tx, writer_rx) = mpsc::channel(32);

        tokio::spawn(writer_task(writer, writer_rx));
        // The reader task that produces ConnectionEvents will be added later.

        Self { writer_tx }
    }

    pub async fn send_control(&self, msg: ControlMsg) -> Result<()> {
        let frame = Frame::Control(msg);
        let bytes = rmp_serde::to_vec(&frame)?;
        self.writer_tx.send(bytes).await?;
        Ok(())
    }
}

async fn writer_task(
    mut writer: WriteHalf<Box<dyn AsyncReadWrite + Send + Unpin>>,
    mut rx: mpsc::Receiver<Vec<u8>>,
) {
    while let Some(bytes) = rx.recv().await {
        let len = bytes.len() as u32;
        if writer.write_all(&len.to_be_bytes()).await.is_err() {
            break;
        }
        if writer.write_all(&bytes).await.is_err() {
            break;
        }
    }
}
