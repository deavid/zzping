//! Scheduler actor for coordinating ping operations.

use actix::prelude::*;
use std::collections::VecDeque;
use std::net::IpAddr;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use zzmem_db::messages::StorePingResult;
use zzmem_db::network_messages::PingResult;

use crate::messages::{
    PingEvent, PingState, SchedulePings, UpdateBackendRecipient, UpdateCState, UpdateIntentConfig,
};

/// Phase angle in degrees (0-360). Currently set to 0 as per design.
/// This controls the offset of ping slots within each second.
const PING_PHASE_DEGREES: u16 = 0;
/// Number of nanoseconds in one second.
const NANOSECONDS_PER_SECOND: u64 = 1_000_000_000;
/// How far ahead (in milliseconds) the scheduler will attempt to dispatch work
/// to the backend. The SyncArbiter thread pool must have at least this many
/// threads to absorb the pending work.
const BACKEND_SCHEDULE_AHEAD_MS: u64 = 5;
/// Maximum number of ping results we allow to buffer locally before pausing the
/// scheduler to let MemDB catch up.
const MAX_PENDING_RESULTS: usize = 1024;

/// Actor that schedules ping operations based on configuration and system clock.
pub struct PingerSchedulerActor {
    targets: Vec<IpAddr>,
    pings_per_second: u16,
    enabled: bool,
    backend_recipient: Option<Recipient<SchedulePings>>,
    memdb_recipient: Recipient<StorePingResult>,
    pending_results: VecDeque<PingResult>,
    memdb_blocked: bool,
    next_ping_slot_time: Option<SystemTime>,
    sequence_counter: u64,
}

impl PingerSchedulerActor {
    /// Creates a new scheduler actor.
    pub fn new(
        backend_recipient: Option<Recipient<SchedulePings>>,
        memdb_recipient: Recipient<StorePingResult>,
    ) -> Self {
        Self {
            targets: Vec::new(),
            pings_per_second: 0,
            enabled: false,
            backend_recipient,
            memdb_recipient,
            pending_results: VecDeque::new(),
            memdb_blocked: false,
            next_ping_slot_time: None,
            sequence_counter: 0,
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
        self.flush_memdb_queue();

        if self.memdb_blocked || self.pending_results.len() >= MAX_PENDING_RESULTS {
            return;
        }

        if self.pings_per_second == 0 {
            self.next_ping_slot_time = None;
            return;
        }

        if self.next_ping_slot_time.is_none() {
            self.next_ping_slot_time =
                Self::compute_next_slot_from(SystemTime::now(), self.pings_per_second);
        }

        let Some(interval_ns) = Self::interval_ns(self.pings_per_second) else {
            return;
        };
        let slot_interval = Duration::from_nanos(interval_ns);
        let schedule_deadline =
            SystemTime::now() + Duration::from_millis(BACKEND_SCHEDULE_AHEAD_MS);

        while let Some(slot_time) = self.next_ping_slot_time {
            if slot_time > schedule_deadline {
                break;
            }

            let fire_duration = match slot_time.duration_since(SystemTime::now()) {
                Ok(delta) => delta,
                Err(_) => Duration::ZERO,
            };

            if self.enabled
                && !self.targets.is_empty()
                && let Some(ref backend_recipient) = self.backend_recipient
            {
                let schedule_msg = SchedulePings {
                    aligned_time: slot_time,
                    instant: Instant::now(),
                    fire_duration,
                    targets: self.targets.clone(),
                    sequence: self.sequence_counter,
                };
                backend_recipient.do_send(schedule_msg);
            }

            self.sequence_counter = self.sequence_counter.wrapping_add(1);
            self.next_ping_slot_time = Some(slot_time + slot_interval);
        }
    }
}

impl Handler<UpdateBackendRecipient> for PingerSchedulerActor {
    type Result = ();

    fn handle(&mut self, msg: UpdateBackendRecipient, _ctx: &mut Self::Context) {
        self.backend_recipient = Some(msg.recipient);
    }
}

impl Handler<UpdateIntentConfig> for PingerSchedulerActor {
    type Result = ();

    fn handle(&mut self, msg: UpdateIntentConfig, _ctx: &mut Self::Context) {
        self.targets = msg.targets;
        self.pings_per_second = msg.pings_per_second;
        self.next_ping_slot_time =
            Self::compute_next_slot_from(SystemTime::now(), self.pings_per_second);
    }
}

impl Handler<UpdateCState> for PingerSchedulerActor {
    type Result = ();

    fn handle(&mut self, msg: UpdateCState, _ctx: &mut Self::Context) {
        self.enabled = msg.enable;
        if self.enabled && self.next_ping_slot_time.is_none() {
            self.next_ping_slot_time =
                Self::compute_next_slot_from(SystemTime::now(), self.pings_per_second);
        }
    }
}

impl Handler<PingEvent> for PingerSchedulerActor {
    type Result = ();

    fn handle(&mut self, msg: PingEvent, _ctx: &mut Self::Context) {
        // Convert PingEvent to the MemDB PingResult representation.
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
            sequence: msg.sequence as u32,
        };

        self.pending_results.push_back(ping_result);
        self.flush_memdb_queue();
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

    fn flush_memdb_queue(&mut self) {
        while let Some(result) = self.pending_results.front().cloned() {
            let store_msg = StorePingResult { result };
            if self.memdb_recipient.try_send(store_msg).is_ok() {
                self.pending_results.pop_front();
            } else {
                self.memdb_blocked = true;
                break;
            }
        }

        if self.pending_results.is_empty() {
            self.memdb_blocked = false;
        }
    }
}
