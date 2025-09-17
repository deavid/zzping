use anyhow::Result;
use async_trait::async_trait;
use log::info;
use std::marker::PhantomData;
use tokio::sync::{mpsc, oneshot};
use zzchorale::{create_channel, spawn_component, Component, ComponentHandle, Wired};

/// The command type for the Pong component.
/// It's a tuple containing the message string and a oneshot sender for the response.
pub type PongCommand = (String, oneshot::Sender<String>);

/// The Pong component. It listens for "ping" messages and responds with "pong".
#[derive(Default)]
pub struct PongComponent;

#[async_trait]
impl Component for PongComponent {
    type Command = PongCommand;

    async fn handle_command(&mut self, command: Self::Command) -> Result<()> {
        let (message, response_tx) = command;
        info!("PongComponent received command: '{message}'");
        if message == "ping" && response_tx.send("pong".to_string()).is_err() {
            info!("PongComponent failed to send response; receiver dropped.");
        }
        Ok(())
    }
}

/// Builder for the PongComponent.
pub struct PongBuilder<State> {
    command_tx: mpsc::Sender<PongCommand>,
    command_rx: Option<mpsc::Receiver<PongCommand>>,
    _state: PhantomData<State>,
}

impl<State> PongBuilder<State> {
    /// Returns a clone of the command sender, allowing other components to communicate with the PongComponent.
    pub fn get_command_sender(&self) -> mpsc::Sender<PongCommand> {
        self.command_tx.clone()
    }
}

impl PongBuilder<Wired> {
    /// Creates a new PongBuilder, internally creating the command channel.
    /// Since Pong has no dependencies, it's considered "Wired" from the start.
    pub fn new() -> Self {
        let (command_tx, command_rx) = create_channel();
        Self {
            command_tx,
            command_rx: Some(command_rx),
            _state: PhantomData,
        }
    }

    /// Consumes the builder to start the component.
    pub async fn start(mut self) -> Result<ComponentHandle<PongCommand>> {
        let component = PongComponent;
        let command_rx = self
            .command_rx
            .take()
            .expect("Builder can only be started once");
        let (handle, readiness) = spawn_component(component, self.command_tx, command_rx);
        readiness.await?;
        Ok(handle)
    }
}

impl Default for PongBuilder<Wired> {
    fn default() -> Self {
        Self::new()
    }
}
