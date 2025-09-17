use crate::connection::{ClientConfig, ServerConfig};
use crate::connection_manager::{Channel, Connection, ConnectionEvent};
use crate::runtime::{client::ClientRuntime, server::ServerRuntime};
use anyhow::Result;
use async_trait::async_trait;
use futures::{stream::StreamExt, Stream};
use log::warn;
use std::{collections::HashMap, pin::Pin, sync::Arc};
use tokio::sync::{mpsc, oneshot, Mutex};
use zzchorale::{create_channel, ComponentHandle};
use zznet_api::{ClientId, ZzRoom};

// Re-export the configs from the connection module
pub use crate::connection::TlsCfg;

/// Top-level configuration for the `ZzNet` component.
#[derive(Clone, Debug)]
pub enum ZzNetConfig {
    Server(ServerConfig),
    Client(ClientConfig),
}

type ListenForRoomResponseTx = oneshot::Sender<Result<mpsc::Receiver<(ClientId, Box<dyn ZzRoom>)>>>;
type RoomListenerTx = mpsc::Sender<(ClientId, Box<dyn ZzRoom>)>;
type GetRoomResponseTx = oneshot::Sender<Result<Box<dyn ZzRoom>>>;

/// Commands that can be sent to the `ZzNetComponent`.
#[derive(Debug)]
pub enum ZzNetCommand {
    ListenForRoom {
        room_name: String,
        response_tx: ListenForRoomResponseTx,
    },
    GetRoom {
        name: String,
        response_tx: GetRoomResponseTx,
    },
    ProcessConnectionEvent(ClientId, ConnectionEvent),
}

// The API for SERVER components
#[async_trait]
pub trait ZzNetServerApi {
    async fn listen_for_room(&self, room_name: String) -> Result<mpsc::Receiver<(ClientId, Box<dyn ZzRoom>)>>;
}

// The API for CLIENT components
#[async_trait]
pub trait ZzNetClientApi {
    async fn get_room(&self, name: &str) -> Result<Box<dyn ZzRoom>>;
}

#[async_trait]
impl ZzNetServerApi for ComponentHandle<ZzNetCommand> {
    async fn listen_for_room(
        &self,
        room_name: String,
    ) -> Result<mpsc::Receiver<(ClientId, Box<dyn ZzRoom>)>> {
        let (response_tx, response_rx) = oneshot::channel();
        let command = ZzNetCommand::ListenForRoom {
            room_name,
            response_tx,
        };
        self.command_tx
            .send(command)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
        response_rx.await?
    }
}

#[async_trait]
impl ZzNetClientApi for ComponentHandle<ZzNetCommand> {
    async fn get_room(&self, name: &str) -> Result<Box<dyn ZzRoom>> {
        let (response_tx, response_rx) = oneshot::channel();
        let command = ZzNetCommand::GetRoom {
            name: name.to_string(),
            response_tx,
        };
        self.command_tx
            .send(command)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
        response_rx.await?
    }
}

/// A builder for the `ZzNetComponent`.
pub struct ZzNetBuilder {
    config: ZzNetConfig,
}

impl ZzNetBuilder {
    /// Creates a new `ZzNetBuilder` with the specified configuration.
    pub fn new(config: ZzNetConfig) -> Self {
        Self { config }
    }

    /// Starts the `ZzNetComponent` and returns a handle for interacting with it.
    pub async fn start(self) -> Result<ComponentHandle<ZzNetCommand>> {
        let (command_tx, command_rx) = create_channel();
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        let handle = ComponentHandle {
            command_tx: command_tx.clone(),
            shutdown_tx: Arc::new(Mutex::new(Some(shutdown_tx))),
            actor_handle: Arc::new(Mutex::new(None)),
        };

        let mut component =
            ZzNetComponent::new(self.config, command_rx, handle.clone(), None);

        component.on_start().await?;

        let actor_task = tokio::spawn(async move {
            let result = component.run(shutdown_rx).await;
            if let Err(e) = component.on_shutdown().await {
                warn!("Error during shutdown: {e}");
            }
            result
        });

        *handle.actor_handle.lock().await = Some(actor_task);

        Ok(handle)
    }
}

type ConnectionStream =
    Pin<Box<dyn Stream<Item = Result<(Connection, mpsc::Receiver<ConnectionEvent>)>> + Send>>;

/// The core actor for the `ZzNet` boundary component.
pub struct ZzNetComponent {
    config: ZzNetConfig,
    command_rx: mpsc::Receiver<ZzNetCommand>,
    handle: ComponentHandle<ZzNetCommand>,
    connection_stream: Option<ConnectionStream>,
    listeners: HashMap<String, RoomListenerTx>,
    connections: HashMap<ClientId, Connection>,
    next_client_id: ClientId,
    open_rooms: HashMap<String, Channel>,
    pending_requests: HashMap<String, Vec<GetRoomResponseTx>>,
}

impl ZzNetComponent {
    /// Creates a new `ZzNetComponent`.
    pub fn new(
        config: ZzNetConfig,
        command_rx: mpsc::Receiver<ZzNetCommand>,
        handle: ComponentHandle<ZzNetCommand>,
        initial_stream: Option<ConnectionStream>,
    ) -> Self {
        Self {
            config,
            command_rx,
            handle,
            connection_stream: initial_stream,
            listeners: HashMap::new(),
            connections: HashMap::new(),
            next_client_id: 1,
            open_rooms: HashMap::new(),
            pending_requests: HashMap::new(),
        }
    }

    pub async fn on_start(&mut self) -> Result<()> {
        if self.connection_stream.is_some() {
            return Ok(());
        }

        let stream: ConnectionStream = match self.config.clone() {
            ZzNetConfig::Server(config) => {
                let s = ServerRuntime::new(config).run().await?;
                Box::pin(s)
            }
            ZzNetConfig::Client(config) => {
                let s = ClientRuntime::new(config).connections();
                Box::pin(s)
            }
        };
        self.connection_stream = Some(stream);
        Ok(())
    }

    pub async fn on_shutdown(&mut self) -> Result<()> {
        Ok(())
    }

    async fn handle_command(&mut self, command: ZzNetCommand) -> Result<()> {
        match command {
            ZzNetCommand::ListenForRoom {
                room_name,
                response_tx,
            } => {
                let (tx, rx) = mpsc::channel(1);
                self.listeners.insert(room_name, tx);
                let _ = response_tx.send(Ok(rx));
            }
            ZzNetCommand::GetRoom { name, response_tx } => {
                if let Some(room) = self.open_rooms.get(&name) {
                    let _ = response_tx.send(Ok(Box::new(room.clone())));
                } else {
                    self.pending_requests
                        .entry(name)
                        .or_default()
                        .push(response_tx);
                }
            }
            ZzNetCommand::ProcessConnectionEvent(client_id, event) => match event {
                ConnectionEvent::ChannelOpened { name, id, receiver } => {
                    let (command_tx, mut command_rx) = mpsc::channel(32);
                    tokio::spawn(async move {
                        while command_rx.recv().await.is_some() {
                            // This is a dummy writer task for now
                        }
                    });
                    let room = Channel::new(id, command_tx, receiver);

                    if let Some(waiters) = self.pending_requests.remove(&name) {
                        for waiter in waiters {
                            let _ = waiter.send(Ok(Box::new(room.clone())));
                        }
                    }

                    if let Some(listener) = self.listeners.get(&name) {
                        let _ = listener.send((client_id, Box::new(room.clone()))).await;
                    }

                    self.open_rooms.insert(name, room);
                }
            },
        }
        Ok(())
    }

    pub async fn run(&mut self, mut shutdown_rx: oneshot::Receiver<()>) -> Result<()> {
        let mut connection_stream = self.connection_stream.take().unwrap();

        loop {
            tokio::select! {
                _ = &mut shutdown_rx => {
                    log::info!("ZzNetComponent received shutdown signal.");
                    break;
                },
                Some(command) = self.command_rx.recv() => {
                    if let Err(e) = self.handle_command(command).await {
                        log::error!("Error handling command: {e}");
                    }
                },
                Some(connection_result) = connection_stream.next() => {
                    match connection_result {
                        Ok((connection, mut event_rx)) => {
                            let client_id = self.next_client_id;
                            self.next_client_id += 1;

                            if let ZzNetConfig::Client(config) = &self.config {
                                for room_name in &config.rooms_to_open {
                                    let msg = crate::proto::ControlMsg::RequestRoom { name: room_name.clone() };
                                    if let Err(e) = connection.send_control(msg).await {
                                        log::error!("Failed to send RequestRoom for {room_name}: {e}");
                                    }
                                }
                            }

                            self.connections.insert(client_id, connection);

                            let handle = self.handle.clone();
                            tokio::spawn(async move {
                                while let Some(event) = event_rx.recv().await {
                                    let cmd = ZzNetCommand::ProcessConnectionEvent(client_id, event);
                                    if handle.command_tx.send(cmd).await.is_err() {
                                        log::warn!("Failed to send event to ZzNetComponent; component might be shutting down.");
                                        break;
                                    }
                                }
                            });
                        },
                        Err(e) => {
                            log::warn!("Failed to establish connection: {e}");
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
