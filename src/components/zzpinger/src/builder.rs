//! Builder module for constructing the zzpinger component.

use actix::Recipient;
use actix::prelude::*;
use tokio::sync::{mpsc, watch};
use zzmem_db::messages::StorePingResult;

use crate::backend;
use crate::scheduler::PingerSchedulerActor;
use crate::traits::{Clock, PingerClient};
use std::sync::Arc;

/// Strategy for spawning the Pinger actor and backend.
#[derive(Clone, Copy, Debug)]
pub enum SpawnStrategy {
    /// Spawn on a new Arbiter thread (production default).
    NewArbiter,
    /// Spawn on the current Arbiter thread (test mode, respects tokio::time::pause).
    Current,
}

/// Builder for creating the Pinger component.
#[derive(Clone)]
pub struct PingerBuilder {
    /// Optional clock for testing.
    pub clock: Option<Arc<dyn Clock>>,
    /// Strategy for spawning the actor and backend.
    pub spawn_strategy: SpawnStrategy,
}

impl PingerBuilder {
    /// Builds and starts the Pinger component on the current Arbiter.
    /// This is the default production method that creates a new Arbiter.
    pub fn start(
        self,
        client: impl PingerClient,
        memdb_recipient: Recipient<StorePingResult>,
    ) -> Addr<PingerSchedulerActor> {
        match self.spawn_strategy {
            SpawnStrategy::NewArbiter => {
                let arbiter = Arbiter::new();
                self.start_on_arbiter(arbiter.handle(), client, memdb_recipient)
            }
            SpawnStrategy::Current => {
                let current_arbiter = Arbiter::current();
                self.start_on_arbiter(current_arbiter, client, memdb_recipient)
            }
        }
    }

    /// Builds and starts the Pinger component on a specific Arbiter.
    /// This allows explicit control over the execution context, useful for testing.
    pub fn start_on_arbiter(
        self,
        arbiter: ArbiterHandle,
        client: impl PingerClient,
        memdb_recipient: Recipient<StorePingResult>,
    ) -> Addr<PingerSchedulerActor> {
        // Create Tokio channels for backend
        let (work_tx, work_rx) = mpsc::channel(100);
        let (state_tx, state_rx) = watch::channel(false);

        let memdb_recipient = memdb_recipient.clone();
        let clock = self.clock.clone();

        // Start the scheduler actor on the provided arbiter
        let pinger_addr = PingerSchedulerActor::start_in_arbiter(&arbiter, move |_| {
            PingerSchedulerActor::new(work_tx, state_tx, memdb_recipient, clock)
        });

        let scheduler_addr = pinger_addr.clone();
        let client_clone = client;
        let clock = self.clock.clone();

        let backend_future = async move {
            backend::run_backend(work_rx, state_rx, scheduler_addr, client_clone, clock).await;
        };

        // Spawn the backend on the same arbiter
        arbiter.spawn(backend_future);

        pinger_addr
    }
}
