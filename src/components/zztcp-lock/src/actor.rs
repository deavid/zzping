//! This module implements the `TcpLockActor` for TCP-based locking.
//!
//! The actor attempts to bind to a specified TCP address. If successful, it holds the
//! listener open, signifying that the lock is acquired. It notifies a recipient
//! about the lock status changes.

use crate::messages::{SetLockDesired, UpdateLockStatus};
use actix::prelude::*;
use std::time::Duration;
use tokio::net::TcpListener;
use tracing::{info, warn};

/// An actor that attempts to acquire and hold a TCP port lock.
///
/// This actor periodically tries to bind to a configured TCP address.
/// If it succeeds, it holds the `TcpListener` open, effectively holding the lock.
/// If it fails (e.g., address in use), it continues to retry.
///
/// It sends `UpdateLockStatus` messages to a recipient whenever the lock status
/// changes (acquired or lost).
pub struct TcpLockActor {
    recipient: Recipient<UpdateLockStatus>,
    bind_addr: String,
    listener: Option<TcpListener>,
    last_reported_locked_status: Option<bool>,
    bind_in_progress: bool,
    /// Whether the actor should be attempting to hold the lock.
    desired: bool,
}

impl TcpLockActor {
    /// Creates a new `TcpLockActor`.
    ///
    /// # Arguments
    ///
    /// * `recipient` - The recipient to notify of lock status changes.
    /// * `bind_addr` - The TCP address to bind to (e.g., "127.0.0.1:7879").
    pub fn new(recipient: Recipient<UpdateLockStatus>, bind_addr: String) -> Self {
        Self {
            recipient,
            bind_addr,
            listener: None,
            last_reported_locked_status: None,
            bind_in_progress: false,
            desired: true, // By default, we desire the lock.
        }
    }

    fn heartbeat(&mut self, ctx: &mut Context<Self>) {
        ctx.run_interval(Duration::from_secs(1), |act, ctx| {
            // Only attempt to bind if we are supposed to, don't have the lock, and are not already trying.
            if act.desired && act.listener.is_none() && !act.bind_in_progress {
                act.bind_in_progress = true;
                let bind_addr = act.bind_addr.clone();

                let fut = async move { TcpListener::bind(&bind_addr).await };

                let fut = fut.into_actor(act).map(|result, act, _ctx| {
                    act.bind_in_progress = false;

                    match result {
                        Ok(listener) => {
                            act.listener = Some(listener);
                            let new_status = true;
                            if act.last_reported_locked_status != Some(new_status) {
                                info!("TCP lock acquired on {}", act.bind_addr);
                                act.recipient.do_send(UpdateLockStatus { locked: new_status });
                                act.last_reported_locked_status = Some(new_status);
                            }
                        }
                        Err(_e) => {
                            // This is an expected failure if another process holds the lock.
                            act.listener = None;
                            let new_status = false;
                            if act.last_reported_locked_status != Some(new_status) {
                                info!("Could not acquire TCP lock on {}. It may be held by another process.", act.bind_addr);
                                act.recipient.do_send(UpdateLockStatus { locked: new_status });
                                act.last_reported_locked_status = Some(new_status);
                            }
                        }
                    }
                });
                ctx.wait(fut);
            }
        });
    }
}

impl Actor for TcpLockActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!(
            "TcpLockActor started. Attempting to lock {}",
            self.bind_addr
        );
        if self.last_reported_locked_status.is_none() {
            self.recipient.do_send(UpdateLockStatus { locked: false });
            self.last_reported_locked_status = Some(false);
        }
        self.heartbeat(ctx);
    }
}

impl Handler<SetLockDesired> for TcpLockActor {
    type Result = ();

    fn handle(&mut self, msg: SetLockDesired, _ctx: &mut Context<Self>) {
        info!("Setting lock desire to: {}", msg.required);
        self.desired = msg.required;

        // If we no longer desire the lock and we currently hold it, release it.
        if !self.desired && self.listener.is_some() {
            warn!("Voluntarily releasing TCP lock on {}", self.bind_addr);
            self.listener = None;
            let new_status = false;
            if self.last_reported_locked_status != Some(new_status) {
                self.recipient
                    .do_send(UpdateLockStatus { locked: new_status });
                self.last_reported_locked_status = Some(new_status);
            }
        }
    }
}
