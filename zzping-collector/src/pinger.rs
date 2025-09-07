use crate::database_client::DatabaseClient;
use crate::ping_client::PingClient;
use anyhow::Result;
use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::{mpsc, Semaphore};
use zzping_proto::zzping::{AnnouncePingsRequest, CollectorRole};

/// A command for the Pinger task.
#[derive(Debug)]
pub enum PingerCommand {
    UpdateRole(CollectorRole),
}

// ... MonotonicTimeSource ...
pub struct MonotonicTimeSource {
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
    pub fn resync(&mut self) {
        self.reference_instant = Instant::now();
        self.reference_system_time = SystemTime::now();
    }
    pub fn now_ns(&mut self) -> Option<u64> {
        let elapsed = self.reference_instant.elapsed();
        let current_system_time = self.reference_system_time + elapsed;
        let now_ns = current_system_time
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("SystemTime is before UNIX_EPOCH")
            .as_nanos() as u64;
        if now_ns < self.last_generated_ns {
            let backward_jump = Duration::from_nanos(self.last_generated_ns - now_ns);
            warn!("Monotonicity violation: System clock may have stepped backwards by {backward_jump:?}. Skipping timestamp.");
            if backward_jump > Duration::from_secs(5) {
                error!("Large backward time jump detected: {backward_jump:?}. This may indicate a critical system clock issue.");
            }
            return None;
        }
        self.last_generated_ns = now_ns;
        Some(now_ns)
    }
}


/// The final, processed result of a ping attempt, ready for the BatchSubmitter.
#[derive(Debug)]
pub struct FinalizedPing {
    pub sent_nanos: u64,
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
    ping_client: Arc<dyn PingClient>,
    results_tx: mpsc::Sender<FinalizedPing>,
    db_client: DatabaseClient,
    time_source: MonotonicTimeSource,
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
        ping_client: Arc<dyn PingClient>,
        results_tx: mpsc::Sender<FinalizedPing>,
        db_client: DatabaseClient,
        command_rx: mpsc::Receiver<PingerCommand>,
    ) -> Self {
        Self {
            target,
            ping_rate_pps,
            grace_period,
            timeout_check_interval,
            ping_client,
            results_tx,
            db_client,
            time_source: MonotonicTimeSource::new(),
            in_flight_pings: HashMap::new(),
            command_rx,
            is_active: false, // Start in a paused state by default
        }
    }

    /// Runs the Pinger's main loop.
    pub async fn run(mut self) -> Result<()> {
        info!(
            "Pinger task started for target {} at {} pps.",
            self.target, self.ping_rate_pps
        );

        if self.ping_rate_pps == 0 {
            info!("Ping rate is 0, pinger for {} will not run.", self.target);
            return Ok(());
        }

        let mut ping_interval =
            tokio::time::interval(Duration::from_secs_f64(1.0 / self.ping_rate_pps as f64));
        ping_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Burst);

        let mut timeout_interval = tokio::time::interval(self.timeout_check_interval);

        let mut sequence_idx: u16 = 0;
        let semaphore = Arc::new(Semaphore::new(self.ping_rate_pps as usize * 2));

        let (internal_tx, mut internal_rx) = mpsc::channel(100);

        loop {
            if self.is_active {
                tokio::select! {
                    _ = ping_interval.tick() => {
                        let permit = match semaphore.clone().try_acquire_owned() {
                            Ok(p) => p,
                            Err(_) => {
                                warn!("Pinger for {} is busy, skipping a ping.", self.target);
                                continue;
                            }
                        };

                        if let Some(sent_nanos) = self.time_source.now_ns() {
                            self.in_flight_pings.insert(sequence_idx, sent_nanos);

                            let mut db_client = self.db_client.clone();
                            tokio::spawn(async move {
                                let request = AnnouncePingsRequest { sent_nanos: vec![sent_nanos] };
                                if let Err(e) = db_client.announce_pings(request).await {
                                    debug!("Failed to announce ping: {e}");
                                }
                            });

                            self.ping_client.ping(sequence_idx, internal_tx.clone(), permit, sent_nanos).await;
                            sequence_idx = sequence_idx.wrapping_add(1);
                        }
                    }
                    Some(command) = self.command_rx.recv() => {
                        self.handle_command(command);
                    }
                    Some(ping_reply) = internal_rx.recv() => {
                        if let Some(sent_nanos) = self.in_flight_pings.remove(&ping_reply.sequence_idx) {
                            let finalized_ping = FinalizedPing { sent_nanos, rtt: ping_reply.rtt };
                            info!("Pinger for {} sending finalized ping: {:?}", self.target, finalized_ping);
                            if self.results_tx.send(finalized_ping).await.is_err() {
                                info!("BatchSubmitter disconnected, Pinger for {} shutting down.", self.target);
                                return Ok(());
                            }
                        } else {
                            warn!("Received result for untracked sequence: {}", ping_reply.sequence_idx);
                        }
                    }
                    _ = timeout_interval.tick() => {
                        if self.handle_timeouts().await.is_err() {
                            return Ok(()); // Error already logged in handle_timeouts
                        }
                    }
                    else => {
                        return Ok(());
                    }
                }
            } else {
                // When not active, only listen for commands.
                if let Some(command) = self.command_rx.recv().await {
                    self.handle_command(command);
                } else {
                    // Channel closed, shut down.
                    info!("Command channel closed, Pinger for {} shutting down.", self.target);
                    return Ok(());
                }
            }
        }
    }

    fn handle_command(&mut self, command: PingerCommand) {
        match command {
            PingerCommand::UpdateRole(role) => {
                let should_be_active =
                    matches!(role, CollectorRole::Primary | CollectorRole::PrimarySupervised);
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
    }

    async fn handle_timeouts(&mut self) -> Result<()> {
        let now_ns = self.time_source.now_ns().unwrap_or(self.time_source.last_generated_ns);
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
                return Ok(()); // Exit the whole function
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn test_monotonic_timestamp_generation() {
        let mut time_source = MonotonicTimeSource::new();
        let t1 = time_source.now_ns().unwrap();
        sleep(Duration::from_millis(10));
        let t2 = time_source.now_ns().unwrap();
        assert!(t2 > t1, "t2 should be greater than t1");
    }

    #[test]
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
