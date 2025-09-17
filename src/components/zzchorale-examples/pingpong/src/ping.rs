use crate::pong::PongCommand;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use log::info;
use tokio::sync::{mpsc, oneshot};
use zzchorale::{create_channel, spawn_component, Component, ComponentHandle};

/// Command enum for the Ping component.
pub enum PingCommand {
    Ping(oneshot::Sender<Result<String>>),
}

/// The Ping component. It sends "ping" messages to the Pong component and forwards the response.
pub struct PingComponent {
    pong_cmd_tx: mpsc::Sender<PongCommand>,
}

#[async_trait]
impl Component for PingComponent {
    type Command = PingCommand;

    async fn handle_command(&mut self, command: Self::Command) -> Result<()> {
        match command {
            PingCommand::Ping(response_tx) => {
                info!("PingComponent received Ping command.");
                let (pong_response_tx, pong_response_rx) = oneshot::channel();
                let pong_command = ("ping".to_string(), pong_response_tx);

                if self.pong_cmd_tx.send(pong_command).await.is_err() {
                    let _ =
                        response_tx.send(Err(anyhow!("Failed to send command to PongComponent")));
                    return Ok(());
                }

                match pong_response_rx.await {
                    Ok(response) => {
                        info!("PingComponent received response: '{response}'");
                        let _ = response_tx.send(Ok(response));
                    }
                    Err(_) => {
                        let _ =
                            response_tx.send(Err(anyhow!("PongComponent dropped response channel")));
                    }
                }
            }
        }
        Ok(())
    }
}

// --- Builder Typestate Implementation ---

/// Holds the builder's state-specific data.
pub struct PingBuilder<State> {
    state: State,
}

/// State data for an unwired PingBuilder.
pub struct PingBuilderUnwired;

/// State data for a wired PingBuilder.
pub struct PingBuilderWired {
    pong_cmd_tx: mpsc::Sender<PongCommand>,
}

impl PingBuilder<PingBuilderUnwired> {
    /// Creates a new, unwired PingBuilder.
    pub fn new() -> Self {
        Self {
            state: PingBuilderUnwired,
        }
    }

    /// Connects the Ping component to the Pong component, transitioning the builder to the `Wired` state.
    pub fn connect_to_pong(
        self,
        pong_cmd_tx: mpsc::Sender<PongCommand>,
    ) -> PingBuilder<PingBuilderWired> {
        PingBuilder {
            state: PingBuilderWired { pong_cmd_tx },
        }
    }
}

impl Default for PingBuilder<PingBuilderUnwired> {
    fn default() -> Self {
        Self::new()
    }
}

impl PingBuilder<PingBuilderWired> {
    /// Spawns the PingComponent and returns a handle to it.
    /// This method is only available on a `Wired` builder, ensuring `pong_cmd_tx` is present.
    pub async fn start(self) -> Result<ComponentHandle<PingCommand>> {
        let component = PingComponent {
            pong_cmd_tx: self.state.pong_cmd_tx,
        };
        let (command_tx, command_rx) = create_channel();
        let (handle, readiness) = spawn_component(component, command_tx, command_rx);
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
