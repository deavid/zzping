// facade.rs
use crate::{client, config::ZzNetConfig, server, ListenerMap};
use anyhow::Result;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use zznet::connection_manager::Connection;
use zznet_api::ZzChannel;

pub struct ZzNet {
    pub(crate) client_connection: Arc<Mutex<Option<Connection>>>,
    pub(crate) server_listeners: Arc<Mutex<ListenerMap>>,
    pub(crate) next_client_id: Arc<Mutex<u64>>,
}

impl ZzNet {
    pub fn new(config: ZzNetConfig) -> Self {
        let net = Self {
            client_connection: Arc::new(Mutex::new(None)),
            server_listeners: Arc::new(Mutex::new(HashMap::new())),
            next_client_id: Arc::new(Mutex::new(0)),
        };

        match config {
            ZzNetConfig::Client(client_config) => {
                client::start_runtime(client_config, Arc::clone(&net.client_connection));
            }
            ZzNetConfig::Server(server_config) => {
                server::start_runtime(
                    server_config,
                    Arc::clone(&net.server_listeners),
                    Arc::clone(&net.next_client_id),
                );
            }
        }
        net
    }

    pub async fn listen_for_channel(
        &self,
        name: &str,
    ) -> Result<mpsc::Receiver<(u64, Box<dyn ZzChannel>)>> {
        let (tx, rx) = mpsc::channel(32);
        self.server_listeners
            .lock()
            .unwrap()
            .insert(name.to_string(), tx);
        Ok(rx)
    }

    pub async fn request_channel(&self, name: String) -> Result<Box<dyn ZzChannel>> {
        // To avoid locking across an await, we clone the connection - that is a TX channel, so we end sending to the same endpoint
        let opt_conn = self.client_connection.lock().unwrap().clone();
        if let Some(conn) = opt_conn {
            let channel = conn.request_channel(name).await?;
            Ok(Box::new(channel))
        } else {
            Err(anyhow::anyhow!("No active connection"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ntest::timeout;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
    use tokio::sync::mpsc as tokio_mpsc;
    use zznet::connection::{ClientConfig, ServerConfig};
    use zznet::connection_manager::Connection;
    use zznet_api::Role;

    // Mock stream implementation copied from zznet's connection_manager tests
    // to allow creating a real Connection object that is backed by mock I/O.
    struct MockStream {
        rx: tokio_mpsc::Receiver<Vec<u8>>,
        tx: tokio_mpsc::Sender<Vec<u8>>,
        buffer: Vec<u8>,
    }

    impl AsyncRead for MockStream {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            if !self.buffer.is_empty() {
                let len = std::cmp::min(buf.remaining(), self.buffer.len());
                buf.put_slice(&self.buffer[..len]);
                self.buffer.drain(..len);
                return Poll::Ready(Ok(()));
            }
            match self.rx.poll_recv(cx) {
                Poll::Ready(Some(data)) => {
                    self.buffer.extend_from_slice(&data);
                    let len = std::cmp::min(buf.remaining(), self.buffer.len());
                    buf.put_slice(&self.buffer[..len]);
                    self.buffer.drain(..len);
                    Poll::Ready(Ok(()))
                }
                Poll::Ready(None) => Poll::Ready(Ok(())),
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
            match self.tx.try_send(buf.to_vec()) {
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
    async fn test_zznet_new_does_not_panic() {
        let _ = env_logger::builder().is_test(true).try_init();
        // This test is trivial. It just ensures that creating a ZzNet instance
        // with a dummy config doesn't panic. The actual runtime behavior
        // is covered by integration tests.
        let dummy_server_config = ServerConfig {
            socketaddr: vec!["127.0.0.1:0".parse().unwrap()],
            tls: None,
            role: Role::Collector,
        };
        let _ = ZzNet::new(ZzNetConfig::Server(dummy_server_config));

        let dummy_client_config = ClientConfig {
            socketaddr: vec!["127.0.0.1:1".parse().unwrap()],
            tls: None,
            role: Role::ClientRo,
            reconnect_delay: std::time::Duration::from_secs(1),
        };
        let _ = ZzNet::new(ZzNetConfig::Client(dummy_client_config));
    }

    #[tokio::test]
    #[timeout(100)]
    async fn test_listen_for_channel() {
        let _ = env_logger::builder().is_test(true).try_init();
        // Manually construct ZzNet to avoid spawning runtimes
        let zznet = ZzNet {
            client_connection: Arc::new(Mutex::new(None)),
            server_listeners: Arc::new(Mutex::new(HashMap::new())),
            next_client_id: Arc::new(Mutex::new(0)),
        };
        let channel_name = "my-channel";

        // Listen for the first time
        let receiver1 = zznet.listen_for_channel(channel_name).await.unwrap();
        assert!(zznet
            .server_listeners
            .lock()
            .unwrap()
            .contains_key(channel_name));

        // Listen again with the same name, should overwrite
        let receiver2 = zznet.listen_for_channel(channel_name).await.unwrap();
        assert!(zznet
            .server_listeners
            .lock()
            .unwrap()
            .contains_key(channel_name));

        // Ensure the receivers are different
        assert_ne!(
            format!("{:?}", receiver1),
            format!("{:?}", receiver2),
            "Listening again should produce a new receiver"
        );
    }

    #[tokio::test]
    #[timeout(100)]
    async fn test_request_channel_no_connection() {
        let _ = env_logger::builder().is_test(true).try_init();
        // Manually construct ZzNet to avoid spawning runtimes
        let zznet = ZzNet {
            client_connection: Arc::new(Mutex::new(None)),
            server_listeners: Arc::new(Mutex::new(HashMap::new())),
            next_client_id: Arc::new(Mutex::new(0)),
        };

        let result = zznet.request_channel("test".to_string()).await;
        match result {
            Ok(_) => panic!("Expected request_channel to fail, but it succeeded."),
            Err(e) => assert_eq!(e.to_string(), "No active connection"),
        }
    }

    #[tokio::test]
    #[timeout(200)]
    async fn test_request_channel_success() {
        // SETUP
        let (tx_to_stream, rx) = tokio_mpsc::channel(32);
        let (tx, mut rx_from_stream) = tokio_mpsc::channel(32);
        let mock_stream = MockStream {
            rx,
            tx,
            buffer: Vec::new(),
        };
        let (event_tx, _event_rx) = tokio_mpsc::channel(32);
        let connection = Connection::new(Box::new(mock_stream), event_tx);

        let zznet = ZzNet {
            client_connection: Arc::new(Mutex::new(Some(connection))),
            server_listeners: Arc::new(Mutex::new(HashMap::new())),
            next_client_id: Arc::new(Mutex::new(0)),
        };

        let channel_name = "test-success".to_string();
        let channel_id = 123;

        // SPAWN PEER SIMULATOR
        // This task will act like the other side of the connection.
        tokio::spawn(async move {
            // 1. Wait for the actor to send its request frame
            let _len = rx_from_stream.recv().await;
            let _data = rx_from_stream.recv().await;

            // 2. Send back a ChannelOpened frame
            let response_frame = zznet::proto::messages::Frame::Control(
                zznet::proto::messages::ControlMsg::ChannelOpened {
                    name: channel_name.clone(),
                    id: channel_id,
                },
            );
            let response_bytes = rmp_serde::to_vec(&response_frame).unwrap();
            let len_bytes = (response_bytes.len() as u32).to_be_bytes();
            let mut framed_response = Vec::new();
            framed_response.extend_from_slice(&len_bytes);
            framed_response.extend_from_slice(&response_bytes);
            tx_to_stream.send(framed_response).await.unwrap();

            // Keep the stream alive for the rest of the test
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        });

        // ACT
        let result = zznet.request_channel("test-success".to_string()).await;

        // ASSERT
        assert!(result.is_ok());
        let channel = result.unwrap();
        // We can't easily check the ID here without making Channel's fields public,
        // but getting an Ok result is a strong indicator of success.
        // As an indirect check, we can see if it's usable.
        assert!(channel.send(vec![1, 2, 3]).await.is_ok());
    }
}
