use crate::pong::PongCommand;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use log::info;
use tokio::sync::{mpsc, oneshot};
use zzchorale::{spawn_actor, Actor, ActorContext, ComponentHandle};

/// Command enum for the Ping component.
pub enum PingCommand {
    Ping(oneshot::Sender<Result<String>>),
}

/// The Ping actor. It sends "ping" messages to the Pong actor and forwards the response.
pub struct PingActor {
    pong_cmd_tx: mpsc::Sender<PongCommand>,
}

#[async_trait]
impl Actor for PingActor {
    type Command = PingCommand;

    async fn run(self, mut context: ActorContext<Self::Command>) -> Result<()> {
        info!("PingActor starting.");
        loop {
            tokio::select! {
                Some(command) = context.command_rx.recv() => {
                    match command {
                        PingCommand::Ping(response_tx) => {
                            info!("PingActor received Ping command.");
                            let (pong_response_tx, pong_response_rx) = oneshot::channel();
                            let pong_command = ("ping".to_string(), pong_response_tx);

                            if self.pong_cmd_tx.send(pong_command).await.is_err() {
                                let _ = response_tx.send(Err(anyhow!("Failed to send command to PongActor")));
                                continue;
                            }

                            match pong_response_rx.await {
                                Ok(response) => {
                                    info!("PingActor received response: '{response}'");
                                    let _ = response_tx.send(Ok(response));
                                }
                                Err(_) => {
                                    let _ = response_tx.send(Err(anyhow!("PongActor dropped response channel")));
                                }
                            }
                        }
                    }
                }
                _ = &mut context.shutdown_rx => {
                    info!("PingActor received shutdown signal.");
                    break;
                }
            }
        }
        info!("PingActor shutting down.");
        Ok(())
    }
}

/// Builder for the PingComponent.
#[derive(Default)]
pub struct PingBuilder {
    pong_cmd_tx: Option<mpsc::Sender<PongCommand>>,
}

impl PingBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Connects the Ping component to the Pong component.
    pub fn connect_to_pong(&mut self, pong_cmd_tx: mpsc::Sender<PongCommand>) {
        self.pong_cmd_tx = Some(pong_cmd_tx);
    }

    /// Spawns the PingActor and returns a handle to it.
    /// Panics if `connect_to_pong` has not been called.
    pub async fn start(self) -> Result<ComponentHandle<PingCommand>> {
        let pong_cmd_tx = self
            .pong_cmd_tx
            .expect("PingBuilder must be connected to Pong before starting.");

        let actor = PingActor { pong_cmd_tx };

        // We need a channel for the PingActor itself.
        let (command_tx, command_rx) = zzchorale::create_channel();
        let (handle, readiness) = spawn_actor(actor, command_tx, command_rx);
        readiness.await?;
        Ok(handle)
    }
}

/// Public API for the Ping component.
#[async_trait]
pub trait PingApi {
    async fn ping(&self) -> Result<String>;
}

#[async_trait]
impl PingApi for ComponentHandle<PingCommand> {
    async fn ping(&self) -> Result<String> {
        let (response_tx, response_rx) = oneshot::channel();
        let command = PingCommand::Ping(response_tx);

        self.command_tx
            .send(command)
            .await
            .map_err(|_| anyhow!("Failed to send Ping command to actor"))?;

        response_rx.await?
    }
}
