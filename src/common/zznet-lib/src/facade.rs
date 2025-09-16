use crate::{
    client, config::ZzNetConfig, server,
};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::collections::HashMap;
use tokio::sync::{mpsc, oneshot};
use zzchorale::{create_actor, Actor, ActorContext, ComponentHandle};
use zznet::connection_manager::{Connection, ConnectionEvent};
use zznet_api::ZzChannel;

// Type aliases for clarity
type ClientChannelData = (u64, Box<dyn ZzChannel>);
type ClientChannelSender = mpsc::Sender<ClientChannelData>;
type ClientChannelReceiver = mpsc::Receiver<ClientChannelData>;
type ListenForChannelResponse = oneshot::Sender<Result<ClientChannelReceiver>>;

// The builder for the ZzNet component.
pub struct ZzNetBuilder {
    pub config: ZzNetConfig,
}

// Actor-specific commands
#[derive(Debug)]
pub enum ActorCommand {
    RequestChannel {
        name: String,
        response: oneshot::Sender<Result<Box<dyn ZzChannel>>>,
    },
    ListenForChannel {
        name: String,
        response: ListenForChannelResponse,
    },
}

// Internal events for communication between connection managers and the actor
#[derive(Debug)]
pub(crate) enum InternalEvent {
    NewClientConnection(Connection),
    NewServerConnection {
        client_id: u64,
        connection: Connection,
        event_rx: mpsc::Receiver<ConnectionEvent>,
    },
    ClientEvent {
        client_id: u64,
        event: ConnectionEvent,
    },
}

// The actor struct
pub(crate) struct ZzNetActor {
    client_connection: Option<Connection>,
    server_connections: HashMap<u64, Connection>,
    listeners: HashMap<String, ClientChannelSender>,
    internal_tx: mpsc::Sender<InternalEvent>,
    internal_rx: mpsc::Receiver<InternalEvent>,
}

impl ZzNetBuilder {
    pub fn new(config: ZzNetConfig) -> Self {
        Self { config }
    }

    // The start method now returns a single, cloneable ComponentHandle.
    pub async fn start(self) -> Result<ComponentHandle<ActorCommand>> {
        let (internal_tx, internal_rx) = mpsc::channel(32);

        let actor = ZzNetActor {
            client_connection: None,
            server_connections: HashMap::new(),
            listeners: HashMap::new(),
            internal_tx: internal_tx.clone(),
            internal_rx,
        };

        let (handle, readiness) = create_actor(actor);

        match self.config {
            ZzNetConfig::Client(config) => {
                client::spawn_connection_manager(config, internal_tx);
            }
            ZzNetConfig::Server(config) => {
                server::spawn_listener(config, internal_tx);
            }
        }

        readiness.await?;
        log::info!("ZzNet component is ready.");

        Ok(handle)
    }
}

#[async_trait]
impl Actor for ZzNetActor {
    type Command = ActorCommand;

    async fn run(mut self, mut context: ActorContext<Self::Command>) -> Result<()> {
        loop {
            tokio::select! {
                Some(command) = context.command_rx.recv() => {
                    self.handle_command(command).await;
                }
                Some(event) = self.internal_rx.recv() => {
                    self.handle_internal_event(event).await;
                }
                _ = &mut context.shutdown_rx => {
                    log::info!("ZzNetActor received shutdown signal. Terminating.");
                    break;
                }
                else => {
                    log::info!("ZzNetActor command and event channels closed. Terminating.");
                    break;
                }
            }
        }
        log::info!("ZzNetActor has shut down.");
        Ok(())
    }
}

impl ZzNetActor {
    async fn handle_command(&mut self, command: ActorCommand) {
        match command {
            ActorCommand::RequestChannel { name, response } => {
                let result = match &self.client_connection {
                    Some(conn) => conn.request_channel(name).await.map(|c| Box::new(c) as Box<dyn ZzChannel>),
                    None => Err(anyhow!("Not connected (client mode)")),
                };
                if response.send(result).is_err() {
                    log::error!("Failed to send RequestChannel response: receiver dropped.");
                }
            }
            ActorCommand::ListenForChannel { name, response } => {
                let (tx, rx) = mpsc::channel(32);
                self.listeners.insert(name, tx);
                if response.send(Ok(rx)).is_err() {
                    log::error!("Failed to send ListenForChannel response: receiver dropped.");
                }
            }
        }
    }

    async fn handle_internal_event(&mut self, event: InternalEvent) {
        log::debug!("Received internal event: {event:?}");
        match event {
            InternalEvent::NewClientConnection(conn) => {
                self.client_connection = Some(conn);
            }
            InternalEvent::NewServerConnection { client_id, connection, event_rx } => {
                self.server_connections.insert(client_id, connection);
                server::spawn_per_client_event_handler(
                    client_id,
                    event_rx,
                    self.internal_tx.clone(),
                );
            }
            InternalEvent::ClientEvent { client_id, event } => {
                let ConnectionEvent::ChannelOpened { name, id, receiver } = event;
                if let Some(connection) = self.server_connections.get(&client_id) {
                    let channel = zznet::connection_manager::Channel {
                        id,
                        command_tx: connection.command_sender(),
                        rx: receiver,
                    };
                    if let Some(listener_tx) = self.listeners.get(&name) {
                        if listener_tx.send((client_id, Box::new(channel) as Box<dyn ZzChannel>)).await.is_err() {
                            log::warn!("A listener for channel '{name}' was dropped.");
                        }
                    }
                }
            }
        }
    }
}

// The public API is now an extension trait on the ComponentHandle.
#[async_trait]
pub trait ZzNetApi {
    async fn request_channel(&self, name: String) -> Result<Box<dyn ZzChannel>>;
    async fn listen_for_channel(
        &self,
        name: String,
    ) -> Result<mpsc::Receiver<(u64, Box<dyn ZzChannel>)>>;
}

#[async_trait]
impl ZzNetApi for ComponentHandle<ActorCommand> {
    async fn request_channel(&self, name: String) -> Result<Box<dyn ZzChannel>> {
        let (response_tx, response_rx) = oneshot::channel();
        self
            .command_tx
            .send(ActorCommand::RequestChannel {
                name,
                response: response_tx,
            })
            .await?;
        response_rx.await?
    }

    async fn listen_for_channel(
        &self,
        name: String,
    ) -> Result<mpsc::Receiver<(u64, Box<dyn ZzChannel>)>> {
        let (response_tx, response_rx) = oneshot::channel();
        self
            .command_tx
            .send(ActorCommand::ListenForChannel {
                name,
                response: response_tx,
            })
            .await?;
        response_rx.await?
    }
}
