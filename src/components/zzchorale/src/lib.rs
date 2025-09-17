use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;

/// A marker type representing a component builder that has not yet been fully wired.
pub struct Unwired;
/// A marker type representing a component builder that has been fully wired and is ready to start.
pub struct Wired;
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
};

/// The core trait for a component.
#[async_trait]
pub trait Component: Sized + Send + 'static {
    type Command: Send + 'static;

    /// Called once, when the component's task is started.
    async fn on_start(&mut self) -> Result<()> {
        Ok(())
    }

    /// Called for each command received. This is the component's primary business logic.
    async fn handle_command(&mut self, command: Self::Command) -> Result<()>;

    /// Called once, just before the component's task is terminated.
    async fn on_shutdown(&mut self) -> Result<()> {
        Ok(())
    }
}

/// A cloneable handle to a running component actor.
pub struct ComponentHandle<C> {
    pub command_tx: mpsc::Sender<C>,
    // The shutdown sender and task handle are wrapped to allow the handle to be cloneable
    // while ensuring the shutdown logic can only be executed once.
    pub shutdown_tx: Arc<Mutex<Option<oneshot::Sender<()>>>>,
    pub actor_handle: Arc<Mutex<Option<JoinHandle<Result<()>>>>>,
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

use std::future::Future;

pub fn spawn_component<C: Component>(
    mut component: C,
    command_tx: mpsc::Sender<C::Command>,
    mut command_rx: mpsc::Receiver<C::Command>,
) -> (ComponentHandle<C::Command>, impl Future<Output = Result<()>> + Send) {
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let (ready_tx, ready_rx) = oneshot::channel();

    let actor_task = tokio::spawn(async move {
        if component.on_start().await.is_err() {
            return Err(anyhow::anyhow!("Component on_start failed"));
        }
        ready_tx.send(()).ok();

        let mut shutdown_rx = shutdown_rx;
        loop {
            tokio::select! {
                Some(cmd) = command_rx.recv() => {
                    if component.handle_command(cmd).await.is_err() {
                        log::error!("Component handle_command failed; shutting down.");
                        break;
                    }
                },
                _ = &mut shutdown_rx => break,
                else => break,
            }
        }
        component.on_shutdown().await
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
    pub async fn shutdown(&self) -> Result<()> {
        if let Some(tx) = self.shutdown_tx.lock().await.take() {
            let _ = tx.send(());
        }

        let handle = self.actor_handle.lock().await.take();

        if let Some(handle) = handle {
            match tokio::time::timeout(std::time::Duration::from_secs(5), handle).await {
                Ok(Ok(Ok(_))) => Ok(()), // Task joined and returned Ok
                Ok(Ok(Err(e))) => Err(e), // Task joined and returned an Err
                Ok(Err(e)) => Err(e.into()), // Task panicked
                Err(_) => Err(anyhow::anyhow!("Component shutdown timed out after 5 seconds")),
            }
        } else {
            Ok(()) // Already shut down
        }
    }
}
