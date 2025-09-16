use anyhow::Result;
use async_trait::async_trait;
use log::info;
use tokio::sync::{mpsc, oneshot};
use zzchorale::{create_channel, spawn_actor, Actor, ActorContext, ComponentHandle};

/// The command type for the Pong component.
/// It's a tuple containing the message string and a oneshot sender for the response.
pub type PongCommand = (String, oneshot::Sender<String>);

/// The Pong actor. It listens for "ping" messages and responds with "pong".
#[derive(Default)]
pub struct PongActor;

#[async_trait]
impl Actor for PongActor {
    type Command = PongCommand;

    async fn run(self, mut context: ActorContext<Self::Command>) -> Result<()> {
        info!("PongActor starting.");
        loop {
            tokio::select! {
                Some((message, response_tx)) = context.command_rx.recv() => {
                    info!("PongActor received command: '{message}'");
                    if message == "ping" && response_tx.send("pong".to_string()).is_err() {
                        info!("PongActor failed to send response; receiver dropped.");
                    }
                }
                _ = &mut context.shutdown_rx => {
                    info!("PongActor received shutdown signal.");
                    break;
                }
            }
        }
        info!("PongActor shutting down.");
        Ok(())
    }
}

/// Builder for the PongComponent.
pub struct PongBuilder {
    command_tx: mpsc::Sender<PongCommand>,
    command_rx: Option<mpsc::Receiver<PongCommand>>,
}

impl PongBuilder {
    /// Creates a new PongBuilder, internally creating the command channel.
    pub fn new() -> Self {
        let (command_tx, command_rx) = create_channel();
        Self {
            command_tx,
            command_rx: Some(command_rx),
        }
    }

    /// Returns a clone of the command sender, allowing other components to communicate with the PongActor.
    pub fn get_command_sender(&self) -> mpsc::Sender<PongCommand> {
        self.command_tx.clone()
    }

    /// Spawns the PongActor and returns a handle to it.
    pub async fn start(mut self) -> Result<ComponentHandle<PongCommand>> {
        let actor = PongActor;
        let command_rx = self
            .command_rx
            .take()
            .expect("Builder can only be started once.");
        let (handle, readiness) = spawn_actor(actor, self.command_tx, command_rx);
        readiness.await?;
        Ok(handle)
    }
}

impl Default for PongBuilder {
    fn default() -> Self {
        Self::new()
    }
}
