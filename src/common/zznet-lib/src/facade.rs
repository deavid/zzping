// facade.rs

use crate::{
    client, config::ZzNetConfig, server,
};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::collections::HashMap;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use zznet::connection_manager::{Channel, Connection, ConnectionEvent};
use zznet_api::ZzChannel;

// Step 1.1: Introduce ZzNetBuilder
pub struct ZzNetBuilder {
    pub config: ZzNetConfig,
}

// A handle for managing the lifecycle of the ZzNet component. It is not cloneable.
#[derive(Debug)]
pub struct ZzNetManager {
    pub actor_handle: JoinHandle<()>,
    command_tx: mpsc::Sender<ActorCommand>, // Keep this for shutdown
}

// A handle for interacting with the ZzNet component. It is cloneable.
#[derive(Debug, Clone)]
pub struct ZzNetHandle {
    command_tx: mpsc::Sender<ActorCommand>,
}

impl ZzNetHandle {
    /// This constructor is for testing purposes only, allowing manual creation of a handle.
    pub fn new(command_tx: mpsc::Sender<ActorCommand>) -> Self {
        Self { command_tx }
    }
}

#[async_trait]
pub trait ZzNet: Send + Sync {
    async fn request_channel(&self, name: String) -> Result<Box<dyn ZzChannel>>;
    async fn listen_for_channel(
        &self,
        name: String,
    ) -> Result<mpsc::Receiver<(u64, Box<dyn ZzChannel>)>>;
}

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

struct ZzNetActor {
    client_connection: Option<Connection>,
    server_connections: HashMap<u64, Connection>,
    listeners: HashMap<String, mpsc::Sender<(u64, Box<dyn ZzChannel>)>>,
    internal_tx: mpsc::Sender<InternalEvent>,
}

#[derive(Debug)]
pub enum ActorCommand {
    RequestChannel {
        name: String,
        response: oneshot::Sender<Result<Box<dyn ZzChannel>>>,
    },
    ListenForChannel {
        name: String,
        response: oneshot::Sender<Result<mpsc::Receiver<(u64, Box<dyn ZzChannel>)>>>,
    },
    Shutdown {
        response: oneshot::Sender<()>,
    },
}

impl ZzNetBuilder {
    pub fn new(config: ZzNetConfig) -> Self {
        Self { config }
    }

    pub async fn start(self) -> Result<(ZzNetManager, ZzNetHandle)> {
        let (command_tx, command_rx) = mpsc::channel(32);
        let (internal_tx, internal_rx) = mpsc::channel(32);
        let (ready_tx, ready_rx) = oneshot::channel();

        let actor = ZzNetActor {
            client_connection: None,
            server_connections: HashMap::new(),
            listeners: HashMap::new(),
            internal_tx: internal_tx.clone(),
        };
        let actor_handle = tokio::spawn(actor.run(command_rx, internal_rx));

        match self.config {
            ZzNetConfig::Client(config) => {
                client::spawn_connection_manager(config, internal_tx, ready_tx);
            }
            ZzNetConfig::Server(config) => {
                server::spawn_listener(config, internal_tx, ready_tx);
            }
        }

        ready_rx.await??;
        log::info!("ZzNet component is ready.");

        let manager = ZzNetManager {
            actor_handle,
            command_tx: command_tx.clone(),
        };
        let handle = ZzNetHandle { command_tx };

        Ok((manager, handle))
    }
}

impl ZzNetActor {
    async fn run(
        mut self,
        mut command_rx: mpsc::Receiver<ActorCommand>,
        mut internal_rx: mpsc::Receiver<InternalEvent>,
    ) {
        loop {
            tokio::select! {
                Some(command) = command_rx.recv() => {
                    if !self.handle_command(command).await {
                        break;
                    }
                }
                Some(event) = internal_rx.recv() => {
                    self.handle_internal_event(event).await;
                }
            }
        }
        log::info!("ZzNetActor is shutting down.");
    }

    async fn handle_command(&mut self, command: ActorCommand) -> bool {
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
            ActorCommand::Shutdown { response } => {
                let _ = response.send(());
                return false;
            }
        }
        true
    }

    async fn handle_internal_event(&mut self, event: InternalEvent) {
        log::debug!("Received internal event: {:?}", event);
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
                    let channel = Channel {
                        id,
                        command_tx: connection.command_sender(),
                        rx: receiver,
                    };
                    if let Some(listener_tx) = self.listeners.get(&name) {
                        if listener_tx.send((client_id, Box::new(channel))).await.is_err() {
                            log::warn!("A listener for channel '{}' was dropped.", name);
                        }
                    }
                }
            }
        }
    }
}

#[async_trait]
impl ZzNet for ZzNetHandle {
    async fn request_channel(&self, name: String) -> Result<Box<dyn ZzChannel>> {
        let (response_tx, response_rx) = oneshot::channel();
        let command = ActorCommand::RequestChannel {
            name,
            response: response_tx,
        };
        self.command_tx.send(command).await?;
        response_rx.await?
    }

    async fn listen_for_channel(
        &self,
        name: String,
    ) -> Result<mpsc::Receiver<(u64, Box<dyn ZzChannel>)>> {
        let (response_tx, response_rx) = oneshot::channel();
        let command = ActorCommand::ListenForChannel {
            name,
            response: response_tx,
        };
        self.command_tx.send(command).await?;
        response_rx.await?
    }
}

impl ZzNetManager {
    pub async fn shutdown(self) -> Result<()> {
        let (response_tx, response_rx) = oneshot::channel();
        let command = ActorCommand::Shutdown { response: response_tx };

        if self.command_tx.send(command).await.is_err() {
            log::warn!("Shutdown command failed: actor already gone.");
        }
        if response_rx.await.is_err() {
            log::warn!("Actor did not acknowledge shutdown: may have crashed.");
        }
        self.actor_handle.await?;
        Ok(())
    }
}
