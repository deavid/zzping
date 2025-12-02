//! This module implements the `TcpLockActor` for TCP-based locking.
//!
//! The actor uses a configurable backend (TCP socket or in-memory) to acquire a lock.
//! Lock acquisition is attempted periodically via the `CheckLock` message.
//! Tests can manually send `CheckLock` messages to force immediate retry without
//! waiting for timers, enabling deterministic testing in `tokio::time::pause()` mode.

use crate::backend::LockBackend;
use crate::config::TcpLockConfig;
use crate::messages::{CheckLock, SetLockDesired, UpdateLockStatus};
use actix::prelude::*;
use std::time::Duration;
use tracing::{debug, info, warn};

/// An actor that attempts to acquire and hold a lock using configurable backends.
///
/// This actor can use either:
/// - **Real TCP Socket (Production):** Binds to a configured TCP address.
/// - **In-Memory Registry (Testing):** Uses a thread-safe atomic set.
///
/// The actor periodically attempts to acquire the lock by sending itself a `CheckLock`
/// message. This design allows tests to manually send `CheckLock` messages in a
/// `tokio::time::pause()` environment, ensuring deterministic behavior.
///
/// Lock status changes are reported to a recipient via `UpdateLockStatus` messages.
pub struct TcpLockActor {
    recipient: Recipient<UpdateLockStatus>,
    config: TcpLockConfig,
    backend: Option<LockBackend>,
    last_reported_locked_status: Option<bool>,
    /// Whether the actor should be attempting to hold the lock.
    desired: bool,
}

impl TcpLockActor {
    /// Creates a new `TcpLockActor` with the given configuration.
    ///
    /// # Arguments
    ///
    /// * `recipient` - The recipient to notify of lock status changes.
    /// * `config` - The lock configuration (strategy and retry interval).
    pub fn new(recipient: Recipient<UpdateLockStatus>, config: TcpLockConfig) -> Self {
        Self {
            recipient,
            config,
            backend: None,
            last_reported_locked_status: None,
            desired: true, // By default, we desire the lock.
        }
    }

    /// Log the current lock strategy for debugging purposes.
    fn log_strategy(&self) {
        match &self.config.strategy {
            crate::config::LockStrategy::Tcp(addr) => {
                info!("Using TCP lock strategy on {}", addr);
            }
            crate::config::LockStrategy::Memory(id) => {
                info!("Using Memory lock strategy with ID: {}", id);
            }
        }
    }

    /// Schedule the next `CheckLock` message.
    /// In production, this fires after retry_interval_ms.
    /// In tests with `tokio::time::pause()`, advancing time triggers this.
    fn schedule_next_check(&self, ctx: &mut Context<Self>) {
        let retry_interval = Duration::from_millis(self.config.retry_interval_ms);
        ctx.run_later(retry_interval, |_act, ctx| {
            // Send CheckLock to trigger the next acquisition attempt.
            ctx.address().do_send(CheckLock);
        });
    }

    /// Attempt to acquire the lock and report status changes.
    fn process_lock_check(&mut self, ctx: &mut Context<Self>) {
        if self.desired && self.backend.is_none() {
            // We want the lock and don't have it. Try to acquire.
            let config = self.config.clone();

            let fut = async move { LockBackend::try_acquire(&config.strategy).await }
                .into_actor(self)
                .map(|result, act, ctx| {
                    match result {
                        Ok(backend) => {
                            act.backend = Some(backend);
                            let new_status = true;
                            if act.last_reported_locked_status != Some(new_status) {
                                info!(
                                    "Lock acquired using {:?}",
                                    match &act.config.strategy {
                                        crate::config::LockStrategy::Tcp(addr) =>
                                            format!("TCP ({})", addr),
                                        crate::config::LockStrategy::Memory(id) =>
                                            format!("Memory ({})", id),
                                    }
                                );
                                act.recipient
                                    .do_send(UpdateLockStatus { locked: new_status });
                                act.last_reported_locked_status = Some(new_status);
                            }
                            act.schedule_next_check(ctx);
                        }
                        Err(e) => {
                            // Lock acquisition failed (expected if another holder exists).
                            debug!("Lock acquisition failed: {}", e);
                            act.backend = None;
                            let new_status = false;
                            if act.last_reported_locked_status != Some(new_status) {
                                info!("Could not acquire lock: {}", e);
                                act.recipient
                                    .do_send(UpdateLockStatus { locked: new_status });
                                act.last_reported_locked_status = Some(new_status);
                            }
                            act.schedule_next_check(ctx);
                        }
                    }
                });
            ctx.wait(fut);
        } else if !self.desired {
            // We don't want the lock anymore. Just schedule the next check in case
            // we want it again later.
            self.schedule_next_check(ctx);
        } else {
            // We already have the lock. Just schedule the next check.
            self.schedule_next_check(ctx);
        }
    }
}

impl Actor for TcpLockActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        self.log_strategy();
        if self.last_reported_locked_status.is_none() {
            self.recipient.do_send(UpdateLockStatus { locked: false });
            self.last_reported_locked_status = Some(false);
        }
        // Send the first CheckLock immediately to start the acquisition loop.
        ctx.address().do_send(CheckLock);
    }
}

impl Handler<CheckLock> for TcpLockActor {
    type Result = ();

    fn handle(&mut self, _msg: CheckLock, ctx: &mut Context<Self>) {
        self.process_lock_check(ctx);
    }
}

impl Handler<SetLockDesired> for TcpLockActor {
    type Result = ();

    fn handle(&mut self, msg: SetLockDesired, ctx: &mut Context<Self>) {
        debug!("Setting lock desire to: {}", msg.required);
        self.desired = msg.required;

        // If we no longer desire the lock and we currently hold it, release it.
        if !self.desired && self.backend.is_some() {
            warn!("Voluntarily releasing lock");
            self.backend = None; // Dropping the backend releases the resource (socket or memory ID).
            let new_status = false;
            if self.last_reported_locked_status != Some(new_status) {
                self.recipient
                    .do_send(UpdateLockStatus { locked: new_status });
                self.last_reported_locked_status = Some(new_status);
            }
        }
        // Immediately schedule the next check so we can respond to the desire change.
        ctx.address().do_send(CheckLock);
    }
}
