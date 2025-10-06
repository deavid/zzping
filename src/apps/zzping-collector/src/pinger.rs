use crate::database_client::DatabaseClientTrait;
use crate::ping_client::PingClient;
use anyhow::Result;
use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::{Semaphore, mpsc};
use zzping_proto::zzping::{AnnouncePingsRequest, CollectorRole};

/// A command for the Pinger task.
#[derive(Debug)]
pub enum PingerCommand {
    /// Update the collector role for this pinger.
    UpdateRole(CollectorRole),
    /// Signal the pinger to stop and exit.
    Shutdown,
}

/// The final, processed result of a ping attempt, ready for the BatchSubmitter.
#[derive(Debug)]
pub struct FinalizedPing {
    /// Monotonic timestamp (nanos) when the ping was sent.
    pub sent_nanos: u64,
    /// RTT in nanoseconds if measured, otherwise None to indicate timeout.
    pub rtt: Option<Duration>,
}

/// The Pinger task is responsible for sending ICMP pings to a single target,
/// generating correct timestamps, tracking in-flight pings, and forwarding
/// final results (including timed-out pings).
pub struct Pinger {
    target: IpAddr,
    ping_rate_pps: u64,
    grace_period: Duration,
    timeout_check_interval: Duration,
    resync_interval: Duration,
    ping_client: Arc<dyn PingClient>,
    results_tx: mpsc::Sender<FinalizedPing>,
    db_client: Arc<dyn DatabaseClientTrait>,
    time_source: Box<dyn TimeSource>,
    in_flight_pings: HashMap<u16, u64>,
    command_rx: mpsc::Receiver<PingerCommand>,
    is_active: bool,
}

impl Pinger {
    /// Creates a new Pinger.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        target: IpAddr,
        ping_rate_pps: u64,
        grace_period: Duration,
        timeout_check_interval: Duration,
        resync_interval: Duration,
        ping_client: Arc<dyn PingClient>,
        results_tx: mpsc::Sender<FinalizedPing>,
        db_client: Arc<dyn DatabaseClientTrait>,
        command_rx: mpsc::Receiver<PingerCommand>,
    ) -> Self {
        Self::new_with_time_source(
            target,
            ping_rate_pps,
            grace_period,
            timeout_check_interval,
            resync_interval,
            ping_client,
            results_tx,
            db_client,
            command_rx,
            Box::new(MonotonicTimeSource::new()),
        )
    }

    /// Testable constructor that allows injecting a TimeSource implementation.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_time_source(
        target: IpAddr,
        ping_rate_pps: u64,
        grace_period: Duration,
        timeout_check_interval: Duration,
        resync_interval: Duration,
        ping_client: Arc<dyn PingClient>,
        results_tx: mpsc::Sender<FinalizedPing>,
        db_client: Arc<dyn DatabaseClientTrait>,
        command_rx: mpsc::Receiver<PingerCommand>,
        time_source: Box<dyn TimeSource>,
    ) -> Self {
        Self {
            target,
            ping_rate_pps,
            grace_period,
            timeout_check_interval,
            resync_interval,
            ping_client,
            results_tx,
            db_client,
            time_source,
            in_flight_pings: HashMap::new(),
            command_rx,
            is_active: false, // Start in a paused state by default
        }
    }

    /// Runs the Pinger's main loop.
    pub async fn run(mut self) -> Result<()> {
        info!("Pinger task started for target {}", self.target);

        let mut ping_interval = if self.ping_rate_pps == 0 {
            // If ping_rate_pps is 0, create an interval that never ticks
            tokio::time::interval(Duration::from_secs(86400)) // 1 day
        } else {
            let mut interval =
                tokio::time::interval(Duration::from_secs_f64(1.0 / self.ping_rate_pps as f64));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Burst);
            interval
        };

        let mut timeout_interval = tokio::time::interval(self.timeout_check_interval);
        // Periodic resync interval for time source (configurable, default 60s)
        let mut resync_interval = tokio::time::interval(self.resync_interval);

        let mut sequence_idx: u16 = 0;
        let semaphore = Arc::new(Semaphore::new(if self.ping_rate_pps == 0 {
            1
        } else {
            self.ping_rate_pps as usize * 2
        }));

        let (internal_tx, mut internal_rx) = mpsc::channel(100);

        loop {
            tokio::select! {
                // This arm is only enabled when the pinger is active and ping_rate_pps > 0.
                _ = ping_interval.tick(), if self.is_active && self.ping_rate_pps > 0 => {
                    let permit = match semaphore.clone().try_acquire_owned() {
                        Ok(p) => p,
                        Err(_) => {
                            warn!("Pinger for {} is busy, skipping a ping.", self.target);
                            continue;
                        }
                    };

                    if let Some(sent_nanos) = self.time_source.now_ns() {
                        self.in_flight_pings.insert(sequence_idx, sent_nanos);

                        let db_client = self.db_client.clone();
                        tokio::spawn(async move {
                            let request = AnnouncePingsRequest { sent_nanos: vec![sent_nanos] };
                            if let Err(e) = db_client.announce_pings(request).await {
                                debug!("Failed to announce ping: {e}");
                            }
                        });

                        self.ping_client.ping(sequence_idx, internal_tx.clone(), permit, sent_nanos).await;
                        sequence_idx = sequence_idx.wrapping_add(1);
                    }
                },

                // These arms are always enabled.
                command = self.command_rx.recv() => {
                    match command {
                        Some(command) => match command {
                            PingerCommand::UpdateRole(role) => {
                                self.update_role(role);
                            }
                            PingerCommand::Shutdown => {
                                info!("SHUTDOWN_LOG: Pinger for {} received shutdown command.", self.target);
                                break;
                            }
                        },
                        None => {
                            // Command sender dropped — exit the loop and shut down.
                            info!("SHUTDOWN_LOG: Pinger for {} command channel closed.", self.target);
                            break;
                        }
                    }
                }
                Some(ping_reply) = internal_rx.recv() => {
                    if let Some(sent_nanos) = self.in_flight_pings.remove(&ping_reply.sequence_idx) {
                        let finalized_ping = FinalizedPing { sent_nanos, rtt: ping_reply.rtt };
                        info!("Pinger for {} sending finalized ping: {:?}", self.target, finalized_ping);
                        if self.results_tx.send(finalized_ping).await.is_err() {
                            info!("BatchSubmitter disconnected, Pinger for {} shutting down.", self.target);
                            break;
                        }
                    } else {
                        warn!("Received result for untracked sequence: {}", ping_reply.sequence_idx);
                    }
                },
                _ = timeout_interval.tick() => {
                    let now_ns = self.time_source.now_ns().unwrap_or(self.time_source.last_generated_ns());
                    let grace_period_ns = self.grace_period.as_nanos() as u64;

                    let mut lost_pings = vec![];
                    self.in_flight_pings.retain(|&_seq, &mut sent_ns| {
                        if now_ns.saturating_sub(sent_ns) > grace_period_ns {
                            lost_pings.push(FinalizedPing {
                                sent_nanos: sent_ns,
                                rtt: None, // Mark as lost
                            });
                            false // Remove from in-flight map
                        } else {
                            true // Keep in map
                        }
                    });

                    for lost_ping in lost_pings {
                        if self.results_tx.send(lost_ping).await.is_err() {
                            info!("BatchSubmitter disconnected while sending lost pings, Pinger for {} shutting down.", self.target);
                            // Break the outer loop
                            break;
                        }
                    }
                }
                // Periodically resync the time source reference pair to limit NTP slew drift.
                _ = resync_interval.tick() => {
                    self.time_source.resync();
                }
                else => {
                    // All channels closed, exit.
                    break;
                }
            }
        }

        info!(
            "SHUTDOWN_LOG: Pinger for {} task shutting down.",
            self.target
        );
        Ok(())
    }

    fn update_role(&mut self, role: CollectorRole) {
        let should_be_active = matches!(
            role,
            CollectorRole::Primary | CollectorRole::PrimarySupervised
        );
        if self.is_active != should_be_active {
            self.is_active = should_be_active;
            info!(
                "Pinger for {} is now {}.",
                self.target,
                if self.is_active { "active" } else { "paused" }
            );
        }
    }
}

/// Trait that abstracts time source behavior for testability.
pub trait TimeSource: Send + Sync {
    /// Return current time in nanos since UNIX_EPOCH, or None if a backward jump
    /// was detected and a caller should skip this tick.
    fn now_ns(&mut self) -> Option<u64>;
    /// Resync the internal reference pair to limit drift.
    fn resync(&mut self);
    /// Expose last generated ns for fallback use.
    fn last_generated_ns(&self) -> u64;
}

/// A source for monotonically increasing timestamps, designed to be resilient
/// to system clock adjustments.
#[derive(Debug, Clone)]
struct MonotonicTimeSource {
    reference_instant: Instant,
    reference_system_time: SystemTime,
    last_generated_ns: u64,
}

impl Default for MonotonicTimeSource {
    fn default() -> Self {
        Self::new()
    }
}
impl MonotonicTimeSource {
    pub fn new() -> Self {
        let now_instant = Instant::now();
        let now_system_time = SystemTime::now();
        let now_ns = now_system_time
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;
        Self {
            reference_instant: now_instant,
            reference_system_time: now_system_time,
            last_generated_ns: now_ns,
        }
    }
    // FIXME: Method not used - why?
    pub fn _resync(&mut self) {
        self.reference_instant = Instant::now();
        self.reference_system_time = SystemTime::now();
    }
    /// Resync the reference pair (SystemTime, Instant). Call periodically.
    pub fn resync(&mut self) {
        self.reference_instant = Instant::now();
        self.reference_system_time = SystemTime::now();
    }

    /// Returns the current timestamp in nanoseconds since UNIX_EPOCH.
    /// If a backward jump larger than 5 seconds is detected, this method will
    /// exit the process immediately to allow an external supervisor to restart it.
    pub fn now_ns(&mut self) -> Option<u64> {
        let elapsed = self.reference_instant.elapsed();
        let current_system_time = self.reference_system_time + elapsed;
        let now_ns = current_system_time
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("SystemTime is before UNIX_EPOCH")
            .as_nanos() as u64;
        if now_ns < self.last_generated_ns {
            let backward_jump = Duration::from_nanos(self.last_generated_ns - now_ns);
            warn!(
                "Monotonicity violation: System clock may have stepped backwards by {backward_jump:?}. Skipping timestamp."
            );
            if backward_jump > Duration::from_secs(5) {
                error!("Large backward time jump detected: {backward_jump:?}.");
                // In tests we do not want to exit the process; instead return None so tests can assert behavior.
                if cfg!(test) {
                    return None;
                } else {
                    error!("Exiting to allow supervisor to restart due to critical clock jump.");
                    // Ensure logs flushed (best effort) then exit.
                    std::process::exit(1);
                }
            }
            return None;
        }
        self.last_generated_ns = now_ns;
        Some(now_ns)
    }
}

impl TimeSource for MonotonicTimeSource {
    fn now_ns(&mut self) -> Option<u64> {
        MonotonicTimeSource::now_ns(self)
    }
    fn resync(&mut self) {
        MonotonicTimeSource::resync(self)
    }
    fn last_generated_ns(&self) -> u64 {
        self.last_generated_ns
    }
}

impl MonotonicTimeSource {
    /// Test helper: set last_generated_ns to simulate a time already generated
    /// in the future. This is intentionally public to allow process-level
    /// tests and examples to exercise the fatal-jump behavior.
    pub fn _set_last_generated_ns_for_testing(&mut self, v: u64) {
        self.last_generated_ns = v;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ntest::timeout;
    use std::thread::sleep;

    #[test]
    #[timeout(100)]
    fn test_monotonic_timestamp_generation() {
        let mut time_source = MonotonicTimeSource::new();
        let t1 = time_source.now_ns().unwrap();
        sleep(Duration::from_millis(10));
        let t2 = time_source.now_ns().unwrap();
        assert!(t2 > t1, "t2 should be greater than t1");
    }

    #[test]
    #[timeout(100)]
    fn test_monotonic_source_handles_backward_jump() {
        let mut time_source = MonotonicTimeSource::new();
        let t1 = time_source.now_ns().unwrap();

        // Simulate a backward clock jump by manually setting the last generated time
        // to a point in the "future".
        time_source.last_generated_ns = t1 + Duration::from_secs(10).as_nanos() as u64;

        // The next call to now_ns should detect the time has gone "backward"
        // relative to the (fake) last generated time, and return None.
        let t2 = time_source.now_ns();
        assert!(
            t2.is_none(),
            "now_ns() should return None when a backward clock jump is detected"
        );
    }
}
