use anyhow::Result;
use async_trait::async_trait;
use std::future::Future;
use std::sync::{Arc, Mutex};
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
};

/// The execution context for an actor, providing channels for commands and shutdown.
pub struct ActorContext<C> {
    pub command_rx: mpsc::Receiver<C>,
    pub shutdown_rx: oneshot::Receiver<()>,
}

/// The core trait for a component actor.
#[async_trait]
pub trait Actor: Send + 'static {
    type Command: Send + 'static;
    async fn run(self, context: ActorContext<Self::Command>) -> Result<()>;
}

/// A cloneable handle to a running component actor.
pub struct ComponentHandle<C> {
    pub command_tx: mpsc::Sender<C>,
    // The shutdown sender and task handle are wrapped to allow the handle to be cloneable
    // while ensuring the shutdown logic can only be executed once.
    shutdown_tx: Arc<Mutex<Option<oneshot::Sender<()>>>>,
    actor_handle: Arc<Mutex<Option<JoinHandle<Result<()>>>>>,
}

// Manual clone implementation to avoid placing an unnecessary `Clone` bound on `C`.
impl<C> Clone for ComponentHandle<C> {
    fn clone(&self) -> Self {
        Self {
            command_tx: self.command_tx.clone(),
            shutdown_tx: Arc::clone(&self.shutdown_tx),
            actor_handle: Arc::clone(&self.actor_handle),
        }
    }
}

/// Creates a new channel for component communication.
/// This is a central place to manage channel creation for the framework.
pub fn create_channel<C>() -> (mpsc::Sender<C>, mpsc::Receiver<C>) {
    mpsc::channel(32)
}

/// Spawns an actor on the tokio runtime with a given command channel.
pub fn spawn_actor<A: Actor>(
    actor: A,
    command_tx: mpsc::Sender<A::Command>,
    command_rx: mpsc::Receiver<A::Command>,
) -> (
    ComponentHandle<A::Command>,
    impl Future<Output = Result<()>> + Send,
) {
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let (ready_tx, ready_rx) = oneshot::channel();

    let context = ActorContext {
        command_rx,
        shutdown_rx,
    };
    let actor_task = tokio::spawn(async move {
        // Signal readiness right before the loop starts
        ready_tx.send(()).ok();
        actor.run(context).await
    });

    let handle = ComponentHandle {
        command_tx,
        shutdown_tx: Arc::new(Mutex::new(Some(shutdown_tx))),
        actor_handle: Arc::new(Mutex::new(Some(actor_task))),
    };

    let readiness_future = async { ready_rx.await.map_err(anyhow::Error::from) };

    (handle, readiness_future)
}

impl<C> ComponentHandle<C> {
    /// Initiates a graceful shutdown of the actor.
    /// This method can be called on any cloned handle, but it will only execute once.
    pub async fn shutdown(&self) -> Result<()> {
        // Take the shutdown sender, ensuring this can only happen once.
        if let Some(tx) = self.shutdown_tx.lock().unwrap().take() {
            // The receiver dropping is a valid shutdown signal, so we don't care if the send fails.
            let _ = tx.send(());
        }

        // Take the actor handle, ensuring it can only be awaited once.
        let handle_to_await = self.actor_handle.lock().unwrap().take();
        if let Some(handle) = handle_to_await {
            // Await the actor's task handle and propagate any panics or errors.
            handle.await??;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ntest::timeout;

    // A dummy actor for testing purposes.
    #[derive(Default)]
    struct TestActor;

    // A dummy command enum for the test actor.
    #[derive(Debug)]
    #[allow(dead_code)] // This is a dummy for testing, so not all variants will be used.
    enum TestCommand {
        DoNothing,
    }

    #[async_trait]
    impl Actor for TestActor {
        type Command = TestCommand;

        async fn run(self, mut context: ActorContext<Self::Command>) -> Result<()> {
            log::info!("TestActor started and running.");
            loop {
                tokio::select! {
                    // Handle commands
                    Some(cmd) = context.command_rx.recv() => {
                        log::info!("TestActor received command: {cmd:?}");
                    },
                    // Handle shutdown signal
                    _ = &mut context.shutdown_rx => {
                        log::info!("TestActor received shutdown signal. Terminating.");
                        break;
                    },
                    // Handle channel closure
                    else => {
                        log::info!("TestActor command channel closed. Terminating.");
                        break;
                    }
                }
            }
            Ok(())
        }
    }

    #[tokio::test]
    #[timeout(100)]
    async fn framework_actor_lifecycle() {
        let _ = env_logger::builder().is_test(true).try_init();
        log::info!("Test starting: framework_actor_lifecycle");

        let actor = TestActor;
        let (command_tx, command_rx) = create_channel::<TestCommand>();
        let (handle, readiness) = spawn_actor(actor, command_tx, command_rx);
        readiness.await.expect("Actor should become ready");
        log::info!("Actor is ready.");

        // Test that the handle is cloneable
        let handle_clone = handle.clone();

        // Shut down using the original handle
        handle
            .shutdown()
            .await
            .expect("Actor should shut down cleanly");
        log::info!("Actor shutdown complete.");

        // Subsequent shutdowns on cloned handles should be no-ops and not panic.
        handle_clone
            .shutdown()
            .await
            .expect("Cloned handle shutdown should be a no-op");
        log::info!("Second shutdown call completed without error.");
    }
}
