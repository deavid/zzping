use crate::messages::UpdateLockStatus;
use actix::prelude::*;
use std::time::Duration;
use tokio::net::TcpListener;
use tracing::info;

pub struct TcpLockActor {
    recipient: Recipient<UpdateLockStatus>,
    bind_addr: String,
    listener: Option<TcpListener>,
    last_reported_locked_status: Option<bool>,
    bind_in_progress: bool, // Flag to prevent concurrent bind attempts
}

impl TcpLockActor {
    pub fn new(recipient: Recipient<UpdateLockStatus>, bind_addr: String) -> Self {
        Self {
            recipient,
            bind_addr,
            listener: None,
            last_reported_locked_status: None,
            bind_in_progress: false,
        }
    }

    fn heartbeat(&mut self, ctx: &mut Context<Self>) {
        ctx.run_interval(Duration::from_secs(1), |act, ctx| {
            // Only attempt to bind if we don't have the lock AND a bind is not already in progress.
            if act.listener.is_none() && !act.bind_in_progress {
                act.bind_in_progress = true;
                let bind_addr = act.bind_addr.clone();

                let fut = async move { TcpListener::bind(&bind_addr).await };

                let fut = fut.into_actor(act).map(|result, act, _ctx| {
                    // Reset the flag once the operation is complete.
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
        info!("TcpLockActor started. Attempting to lock {}", self.bind_addr);
        // On startup, we don't have the lock. Report this immediately.
        if self.last_reported_locked_status.is_none() {
             self.recipient.do_send(UpdateLockStatus { locked: false });
             self.last_reported_locked_status = Some(false);
        }
        self.heartbeat(ctx);
    }
}
