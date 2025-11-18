//! Backend module: pure async ping execution in dedicated OS thread.

use crate::messages::{PingEvent, PingState, SchedulePings};
use crate::scheduler::PingerSchedulerActor;
use crate::traits::{Clock, PingError, PingerClient, SystemClock};
use actix::Addr;
use futures::stream::{FuturesUnordered, StreamExt};
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, watch};

/// Executes a single ping operation and returns the final result.
async fn execute_ping(
    target: IpAddr,
    seq: u16,
    client: &impl PingerClient,
    scheduler_addr: Addr<PingerSchedulerActor>,
    clock: Arc<dyn Clock>,
) -> Option<PingEvent> {
    // Send InFlight event immediately
    match scheduler_addr.try_send(PingEvent {
        target_host: target,
        sent_time: clock.now(),
        state: PingState::InFlight,
    }) {
        Ok(_) => {}
        Err(actix::prelude::SendError::Full(_)) => {
            log::warn!("Scheduler mailbox full, dropping InFlight event");
        }
        Err(actix::prelude::SendError::Closed(_)) => {
            panic!("Scheduler actor died");
        }
    }

    // Capture precise send time immediately before ping
    let actual_send_time = clock.now();

    let final_state = match client.ping(target, seq).await {
        Ok(duration) => PingState::ReceivedRTT(duration),
        Err(PingError::Timeout) => PingState::TimedOut,
        Err(PingError::NetworkError) => PingState::NetworkError,
    };

    Some(PingEvent {
        target_host: target,
        sent_time: actual_send_time,
        state: final_state,
    })
}

/// Runs the background ping execution backend.
///
/// This is a dedicated async task running on a dedicated thread via Arbiter.
/// It receives ping commands via `work_rx` and sends results directly to the scheduler actor.
/// It respects the enabled/disabled state via `state_rx`.
pub(crate) async fn run_backend(
    mut work_rx: mpsc::Receiver<SchedulePings>,
    state_rx: watch::Receiver<bool>,
    scheduler_addr: Addr<PingerSchedulerActor>,
    client: impl PingerClient,
    clock: Option<Arc<dyn Clock>>,
) {
    let mut futures = FuturesUnordered::new();
    let mut sequence_numbers: HashMap<IpAddr, u16> = HashMap::new();
    let mut last_error_log = Instant::now() - Duration::from_secs(6); // Allow immediate first log
    let clock = clock.unwrap_or_else(|| Arc::new(SystemClock));

    loop {
        tokio::select! {
            Some(message) = work_rx.recv() => {
                // Check state
                if !*state_rx.borrow() {
                    continue;
                }

                // High-precision wait
                let sleep_until = tokio::time::Instant::from_std(message.instant) + message.fire_duration;
                tokio::time::sleep_until(sleep_until).await;

                // Concurrent spawning
                for target in message.targets {
                    let sequence = sequence_numbers.entry(target).or_insert(0);
                    *sequence = sequence.wrapping_add(1);

                    let seq = *sequence;
                    let clock = clock.clone();

                    futures.push(execute_ping(target, seq, &client, scheduler_addr.clone(), clock));
                }
            }
            Some(opt_event) = futures.next() => {
                // Send final ping result
                if let Some(event) = opt_event {
                    // Muffler: rate limit NetworkError logging
                    if matches!(event.state, PingState::NetworkError) {
                        let now = Instant::now();
                        if now > last_error_log + Duration::from_secs(5) {
                            log::error!("Network error occurred for target {}", event.target_host);
                            last_error_log = now;
                        }
                    }
                    scheduler_addr.do_send(event);
                }
            }
            else => {
                // If work_rx has closed and futures has nothing remaining we can close the backend
                log::info!("Closing ping backend");
                break;
            },
        }
    }
}
