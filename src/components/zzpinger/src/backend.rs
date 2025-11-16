//! Backend actor for executing ping operations.

use actix::prelude::*;
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::hint::spin_loop;
use std::net::IpAddr;
use std::time::{Duration, Instant};
use surge_ping::{Client, Config, PingIdentifier, PingSequence, Pinger};
use tokio::runtime::{Builder, Runtime};
use tokio::time::timeout;
use tracing::warn;

use crate::messages::{PingEvent, PingState, SchedulePings};

/// Error types for ping operations.
#[derive(Debug)]
enum PingError {
    Timeout,
    NetworkError,
}

/// Actor that handles the execution of ping operations.
pub struct PingerBackendActor {
    event_recipient: Recipient<PingEvent>,
    client: Client,
    /// Cache of Pinger objects per target to avoid repeated socket creation.
    pingers: HashMap<IpAddr, Pinger>,
    /// Shared runtime to drive async ping futures from the sync context.
    runtime: Runtime,
}

impl PingerBackendActor {
    /// Creates a new backend actor with the given event recipient.
    pub fn new(event_recipient: Recipient<PingEvent>) -> Self {
        let client = Client::new(&Config::default()).expect("Failed to create ping client");
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create shared tokio runtime");
        Self {
            event_recipient,
            client,
            pingers: HashMap::new(),
            runtime,
        }
    }
}

impl Actor for PingerBackendActor {
    type Context = SyncContext<Self>;
}

impl Handler<SchedulePings> for PingerBackendActor {
    type Result = ();

    fn handle(&mut self, msg: SchedulePings, _ctx: &mut Self::Context) {
        // Wait until the fire time
        let fire_instant = msg.instant + msg.fire_duration;
        Self::wait_until(fire_instant);

        // Perform pings for each target
        for target in msg.targets {
            let event_recipient = self.event_recipient.clone();
            let aligned_time = msg.aligned_time;
            let sequence = msg.sequence;

            // Send InFlight event
            event_recipient.do_send(PingEvent {
                target_host: target,
                sent_time: aligned_time,
                state: PingState::InFlight,
                sequence,
            });

            // Perform the ping synchronously
            match self.perform_ping(target, sequence) {
                Ok(rtt) => {
                    event_recipient.do_send(PingEvent {
                        target_host: target,
                        sent_time: aligned_time,
                        state: PingState::ReceivedRTT(rtt),
                        sequence,
                    });
                }
                Err(PingError::Timeout) => {
                    event_recipient.do_send(PingEvent {
                        target_host: target,
                        sent_time: aligned_time,
                        state: PingState::TimedOut,
                        sequence,
                    });
                }
                Err(PingError::NetworkError) => {
                    event_recipient.do_send(PingEvent {
                        target_host: target,
                        sent_time: aligned_time,
                        state: PingState::NetworkError,
                        sequence,
                    });
                }
            }
        }
    }
}

impl PingerBackendActor {
    fn wait_until(deadline: Instant) {
        const SPIN_THRESHOLD: Duration = Duration::from_micros(200);
        loop {
            match deadline.checked_duration_since(Instant::now()) {
                Some(remaining) if remaining > SPIN_THRESHOLD => {
                    std::thread::sleep(remaining - SPIN_THRESHOLD);
                }
                Some(_) => spin_loop(),
                None => break,
            }
        }
    }

    fn ensure_pinger(&mut self, target: IpAddr) -> Result<(), PingError> {
        if self.pingers.contains_key(&target) {
            return Ok(());
        }

        let identifier = Self::identifier_for(target);
        let pinger = self
            .runtime
            .block_on(self.client.pinger(target, identifier));
        self.pingers.insert(target, pinger);
        Ok(())
    }

    fn identifier_for(target: IpAddr) -> PingIdentifier {
        let mut hasher = DefaultHasher::new();
        target.hash(&mut hasher);
        // Avoid identifier 0 to reduce ambiguity.
        let value = (hasher.finish() as u16).max(1);
        PingIdentifier(value)
    }

    /// Performs a single ping to the target with a 10-second timeout.
    ///
    /// The Pinger objects are cached to avoid repeated socket creation.
    fn perform_ping(&mut self, target: IpAddr, sequence: u64) -> Result<Duration, PingError> {
        self.ensure_pinger(target)?;

        let payload = vec![0; 56];
        let ping_sequence = PingSequence((sequence & u16::MAX as u64) as u16);

        let pinger = self
            .pingers
            .get_mut(&target)
            .expect("pinger must exist after ensure_pinger");
        self.runtime.block_on(async {
            match timeout(
                Duration::from_secs(10),
                pinger.ping(ping_sequence, &payload),
            )
            .await
            {
                Ok(Ok((_seq, rtt))) => Ok(rtt),
                Ok(Err(err)) => {
                    warn!(target = ?target, error = ?err, "ping failed");
                    Err(PingError::NetworkError)
                }
                Err(_) => Err(PingError::Timeout),
            }
        })
    }
}
