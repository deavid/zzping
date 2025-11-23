//! Scheduler actor for coordinating ping operations.

use actix::prelude::*;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::net::IpAddr;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, watch};
use zzmem_db::PingResult;
use zzmem_db::StorePingResult;

use crate::messages::{PingEvent, PingState, SchedulePings, UpdateCState, UpdateIntentConfig};
use crate::traits::{Clock, SystemClock};
use std::sync::Arc;

/// Phase angle in degrees (0-360). Currently set to 0 as per design.
/// This controls the offset of ping slots within each second.
const PING_PHASE_DEGREES: u16 = 0;
/// Number of nanoseconds in one second.
const NANOSECONDS_PER_SECOND: u64 = 1_000_000_000;
/// Maximum number of pending results before dropping oldest.
const MAX_PENDING_RESULTS: usize = 10_000;
/// Threshold to pause scheduling when pending results exceed this.
const BACKPRESSURE_PAUSE_THRESHOLD: usize = 1_000;
/// Threshold to resume scheduling when pending results drop below this.
const BACKPRESSURE_RESUME_THRESHOLD: usize = 100;

/// Actor that schedules ping operations based on configuration and system clock.
pub struct PingerSchedulerActor {
    targets: Vec<IpAddr>,
    pings_per_second: u16,
    backend_tx: mpsc::Sender<SchedulePings>,
    /// Sender for state updates (enable/disable). Passed to backend on startup.
    state_tx: watch::Sender<bool>,
    memdb_recipient: Recipient<StorePingResult>,
    next_slot: HashMap<IpAddr, Option<SystemTime>>,
    pending_results: VecDeque<StorePingResult>,
    memdb_blocked: bool,
    clock: Arc<dyn Clock>,
}

impl PingerSchedulerActor {
    /// Creates a new scheduler actor.
    pub fn new(
        backend_tx: mpsc::Sender<SchedulePings>,
        state_tx: watch::Sender<bool>,
        memdb_recipient: Recipient<StorePingResult>,
        clock: Option<Arc<dyn Clock>>,
    ) -> Self {
        Self {
            targets: vec![],
            pings_per_second: 0,
            backend_tx,
            state_tx,
            memdb_recipient,
            next_slot: HashMap::new(),
            pending_results: VecDeque::new(),
            memdb_blocked: false,
            clock: clock.unwrap_or_else(|| Arc::new(SystemClock)),
        }
    }
}

impl PingerSchedulerActor {
    fn flush_pending_results(&mut self) {
        while let Some(result) = self.pending_results.front() {
            match self.memdb_recipient.try_send(result.clone()) {
                Ok(_) => {
                    self.pending_results.pop_front();
                }
                Err(actix::prelude::SendError::Full(_)) => {
                    // Pipe is clogged, stop flushing
                    break;
                }
                Err(actix::prelude::SendError::Closed(_)) => {
                    // MemDB actor is dead
                    panic!("Critical dependency MemDB lost");
                }
            }
        }
    }
}

impl Actor for PingerSchedulerActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        // Schedule a 1ms interval tick
        ctx.run_interval(Duration::from_millis(1), |act, _ctx| {
            act.handle_tick();
        });
    }
}

impl PingerSchedulerActor {
    fn handle_tick(&mut self) {
        // Flush pending results at the start
        self.flush_pending_results();

        // Hysteresis check
        if self.pending_results.len() < BACKPRESSURE_RESUME_THRESHOLD {
            self.memdb_blocked = false;
        } else if self.pending_results.len() > BACKPRESSURE_PAUSE_THRESHOLD {
            self.memdb_blocked = true;
        }

        // Block check
        if self.memdb_blocked {
            // Invalidate the schedule so it restarts from now when we resume
            for slot in self.next_slot.values_mut() {
                *slot = None;
            }
            return;
        }

        if self.pings_per_second == 0 {
            return;
        }

        let now = self.clock.now();
        let deadline = now + Duration::from_millis(10);
        let mut bucket: BTreeMap<SystemTime, Vec<IpAddr>> = BTreeMap::new();

        for target in &self.targets {
            let next_slot = self.next_slot.entry(*target).or_insert_with(|| {
                Some(Self::compute_next_slot_from(now, self.pings_per_second).unwrap_or(now))
            });

            // If the slot is None (was cleared during backpressure), reinitialize it
            if next_slot.is_none() {
                *next_slot =
                    Some(Self::compute_next_slot_from(now, self.pings_per_second).unwrap_or(now));
            }

            if let Some(slot) = next_slot {
                while *slot <= deadline {
                    bucket.entry(*slot).or_default().push(*target);
                    *slot = Self::advance_slot(*slot, self.pings_per_second);
                }
            }
        }

        for (aligned_time, targets) in bucket {
            let fire_duration = match aligned_time.duration_since(now) {
                Ok(delta) => delta,
                Err(_) => Duration::ZERO,
            };

            let schedule_msg = SchedulePings {
                aligned_time,
                instant: Instant::now(),
                fire_duration,
                targets,
            };
            let backend_tx = self.backend_tx.clone();
            match backend_tx.try_send(schedule_msg) {
                Ok(_) => {}
                Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                    log::warn!("Backend overloaded");
                }
                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                    panic!("Backend thread died");
                }
            }
        }
    }
}

impl PingerSchedulerActor {
    fn advance_slot(slot: SystemTime, pings_per_second: u16) -> SystemTime {
        let interval_ns = NANOSECONDS_PER_SECOND / pings_per_second as u64;
        slot + Duration::from_nanos(interval_ns)
    }
}

impl Handler<UpdateIntentConfig> for PingerSchedulerActor {
    type Result = ();

    fn handle(&mut self, msg: UpdateIntentConfig, _ctx: &mut Self::Context) {
        self.targets = msg.targets.clone();
        self.pings_per_second = msg.pings_per_second;

        // Initialize next_slot for new targets
        for target in &msg.targets {
            self.next_slot
                .entry(*target)
                .or_insert(Some(self.clock.now()));
        }

        // Remove slots for targets no longer in config
        self.next_slot
            .retain(|target, _| msg.targets.contains(target));
    }
}

impl Handler<UpdateCState> for PingerSchedulerActor {
    type Result = ();

    fn handle(&mut self, msg: UpdateCState, _ctx: &mut Self::Context) {
        // CState controls enable/disable for mastership; it does NOT affect MemDB availability.
        // MemDB availability is determined purely by send success/failure.
        // Broadcast the state update to the backend
        let _ = self.state_tx.send(msg.enable);
    }
}

impl Handler<PingEvent> for PingerSchedulerActor {
    type Result = ();

    fn handle(&mut self, msg: PingEvent, _ctx: &mut Self::Context) {
        // We only populate rtt_us for ReceivedRTT; otherwise it's None.
        let rtt_us = match msg.state {
            PingState::ReceivedRTT(duration) => Some(duration.as_micros() as u32),
            _ => None,
        };

        let ping_result = PingResult {
            target: msg.target_host.to_string(),
            timestamp_ms: msg
                .sent_time
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_millis() as u64,
            rtt_us,
        };

        let store_msg = StorePingResult {
            result: ping_result,
        };

        self.pending_results.push_back(store_msg);

        // Safety cap
        if self.pending_results.len() > MAX_PENDING_RESULTS {
            self.pending_results.pop_front();
            log::warn!("Dropping result due to buffer overflow");
        }

        self.flush_pending_results();
    }
}

impl PingerSchedulerActor {
    fn interval_ns(pings_per_second: u16) -> Option<u64> {
        if pings_per_second == 0 {
            None
        } else {
            Some(NANOSECONDS_PER_SECOND / pings_per_second as u64)
        }
    }

    fn compute_next_slot_from(now: SystemTime, pings_per_second: u16) -> Option<SystemTime> {
        let interval_ns = Self::interval_ns(pings_per_second)?;
        let phase_offset_ns = interval_ns * (PING_PHASE_DEGREES as u64) / 360;
        let since_epoch = now.duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO);
        let mut secs = since_epoch.as_secs();
        let nanos_of_second = since_epoch.subsec_nanos() as u64;

        let mut slot_ns = if nanos_of_second <= phase_offset_ns {
            phase_offset_ns
        } else {
            let delta = nanos_of_second - phase_offset_ns;
            let slots_passed = (delta / interval_ns) + 1;
            phase_offset_ns + slots_passed * interval_ns
        };

        while slot_ns >= NANOSECONDS_PER_SECOND {
            slot_ns -= NANOSECONDS_PER_SECOND;
            secs += 1;
        }

        Some(UNIX_EPOCH + Duration::from_secs(secs) + Duration::from_nanos(slot_ns))
    }
}
