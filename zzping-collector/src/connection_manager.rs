//! Manages the state for a single, active connection to the `zzping-database`.

use crate::pinger::{ping_task, PingResult};
use anyhow::Result;
use log::{debug, error};
use std::sync::Arc;
use std::time::Duration;
use surge_ping::{Client, Config, PingIdentifier};
use tokio::io::AsyncWrite;
use tokio::sync::{mpsc, Semaphore};
use tokio::time::{Instant, Interval};
use zzping_lib::protocol::{write_record, RawDataRecord};

struct PingerSession<W>
where
    W: AsyncWrite + Unpin + Send,
{
    cli: Arc<crate::Cli>,
    db_stream: W,
    // This is an Option because the ping client requires privileged access to create
    // a raw socket, which may not be available when running tests. We lazily
    // initialize it on the first ping attempt.
    pinger_client: Option<Client>,
    pinger_ident: PingIdentifier,
    ping_semaphore: Arc<Semaphore>,
    tx: mpsc::Sender<PingResult>,
    rx: mpsc::Receiver<PingResult>,
    sequence_idx: u16,
    interval: Interval,
    start_time_monotonic: Instant,
}

impl<W> PingerSession<W>
where
    W: AsyncWrite + Unpin + Send,
{
    /// Creates a new `PingerSession`.
    ///
    /// This constructor initializes the `surge-ping` client and all the state
    /// required for a new pinging session.
    pub fn new(db_stream: W, cli: Arc<crate::Cli>) -> Result<Self> {
        let pinger_ident = PingIdentifier(rand::random());
        let ping_semaphore = Arc::new(Semaphore::new(cli.max_in_flight));
        let (tx, rx) = mpsc::channel(1024);
        let interval_duration = Duration::from_nanos(1_000_000_000 / cli.rate);
        let interval = tokio::time::interval(interval_duration);
        let start_time_monotonic = Instant::now();

        Ok(Self {
            cli,
            db_stream,
            pinger_client: None,
            pinger_ident,
            ping_semaphore,
            tx,
            rx,
            sequence_idx: 0,
            interval,
            start_time_monotonic,
        })
    }

    /// Performs a single iteration of the event loop.
    ///
    /// This method waits for one of two events:
    /// 1. The interval timer ticks, triggering a new ping to be sent.
    /// 2. A ping result is received from a completed `ping_task`.
    ///
    /// It returns `Ok(())` to indicate the session should continue, or an `Err`
    /// if a fatal error occurs (e.g., the connection to the database is lost).
    pub async fn tick(&mut self) -> Result<()> {
        tokio::select! {
            _ = self.interval.tick() => {
                if let Ok(permit) = self.ping_semaphore.clone().try_acquire_owned() {
                    // Lazily initialize the pinger client, but only after we've
                    // acquired a permit. This avoids attempting a privileged
                    // operation (raw socket creation) in unit tests where we
                    // pre-acquire all permits to test the semaphore logic.
                    if self.pinger_client.is_none() {
                        let pinger_config = Config::default();
                        match Client::new(&pinger_config) {
                            Ok(client) => {
                                self.pinger_client = Some(client);
                            }
                            Err(e) => {
                                error!("Failed to create ping client, likely due to missing privileges: {e}");
                                // Drop the permit and return the error.
                                return Err(e.into());
                            }
                        }
                    }

                    let pinger = self.pinger_client.as_ref().unwrap().pinger(self.cli.target, self.pinger_ident).await;
                    let tx_clone = self.tx.clone();
                    tokio::spawn(ping_task(pinger, self.sequence_idx, tx_clone, permit));
                    self.sequence_idx = self.sequence_idx.wrapping_add(1);
                } else {
                    debug!("Max in-flight pings reached, skipping this tick.");
                }
            }
            Some(ping_result) = self.rx.recv() => {
                let sent_nanos = self.start_time_monotonic.elapsed().as_nanos() as u64;
                let rtt_nanos = ping_result.rtt.map_or(u64::MAX, |rtt| rtt.as_nanos() as u64);

                let record = RawDataRecord {
                    sent_nanos,
                    rtt_nanos,
                };

                if let Err(e) = write_record(&mut self.db_stream, &record).await {
                    error!("Failed to write record to stream: {e}. Disconnecting.");
                    return Err(e);
                }
            }
            else => {
                // Channel closed, which means something went wrong. Terminate the session.
                return Err(anyhow::anyhow!("MPSC channel closed unexpectedly."));
            }
        }
        Ok(())
    }
}

/// Manages the lifecycle of a single connection to the database.
///
/// This function contains the main loop for a connected session. It is responsible for:
/// 1. Setting up the `surge-ping` client.
/// 2. Spawning `ping_task`s at the rate specified by the CLI arguments.
/// 3. Receiving `PingResult`s from the ping tasks.
/// 4. Converting results into `RawDataRecord`s.
/// 5. Serializing records and sending them over the TCP stream to the database.
///
/// If any error occurs while writing to the TCP stream, this function will return,
/// causing the main loop in `main.rs` to attempt a reconnection.
///
/// # Arguments
/// * `db_stream` - An async writer, typically the TCP stream to the `zzping-database`.
/// * `cli` - The parsed command-line arguments.
pub async fn handle_connection<W>(db_stream: W, cli: Arc<crate::Cli>) -> Result<()>
where
    W: AsyncWrite + Unpin + Send,
{
    let mut session = PingerSession::new(db_stream, cli)?;

    loop {
        if let Err(e) = session.tick().await {
            // The tick method returns an error if the session should be terminated.
            // We'll log it and then the function will exit, causing the main loop
            // to attempt a reconnection.
            error!("Pinging session ended with error: {e}");
            return Err(e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Cli;
    use tokio::io::AsyncReadExt;
    use zzping_lib::protocol::read_record;

    /// This test verifies the core logic of the `PingerSession`.
    /// It checks that when a `PingResult` is received (simulating a completed
    /// ping), the session correctly serializes it into a `RawDataRecord` and
    /// writes it to the output stream.
    #[tokio::test]
    async fn pinger_session_tick_writes_record_on_response() -> Result<()> {
        // 1. Setup: Create a PingerSession with a mock writer (a Vec<u8>).
        // We use a high ping rate to ensure the interval timer in `tick()`
        // doesn't fire and interfere with the test.
        let cli = Arc::new(Cli {
            target: "127.0.0.1".parse().unwrap(),
            rate: 10_000, // 10k pings/sec, interval is 0.1ms
            max_in_flight: 4,
            database_addr: "127.0.0.1:7878".to_string(),
        });

        // The buffer will act as our in-memory "network stream".
        let mut buffer: Vec<u8> = Vec::new();
        let mut session = PingerSession::new(&mut buffer, Arc::clone(&cli))?;

        // 2. Action: Manually send a PingResult to the session's internal channel.
        // This simulates a response from a `ping_task`.
        let test_rtt = Duration::from_millis(50);
        session
            .tx
            .send(PingResult { rtt: Some(test_rtt) })
            .await
            .expect("Test receiver should not be closed");

        // 3. Execution: Call `tick()` to process the waiting message.
        // The `select!` in `tick()` should immediately pick the `rx.recv()`
        // branch because we just sent a message.
        session.tick().await?;

        // 4. Verification: Check that the correct data was written to the buffer.
        // We use the `read_record` helper from `zzping-lib` to parse the buffer,
        // ensuring the data is in the expected format.
        let mut cursor = std::io::Cursor::new(&buffer);
        let received_record = read_record(&mut cursor)
            .await
            .expect("Reading record from buffer should not fail")
            .expect("Buffer should contain a full record, not be empty");

        // `sent_nanos` is based on the session's `Instant::now()`, so we can't
        // assert an exact value. We just check that it's plausible.
        assert!(received_record.sent_nanos > 0);

        // The RTT should be exactly what we sent.
        assert_eq!(
            received_record.rtt_nanos,
            test_rtt.as_nanos() as u64,
            "RTT in the written record should match the test RTT"
        );

        // Check that the buffer has been fully consumed by `read_record`.
        // This ensures no extra data was written.
        let mut remaining_bytes = vec![];
        let bytes_read = cursor.read_to_end(&mut remaining_bytes).await?;
        assert_eq!(
            bytes_read, 0,
            "Expected buffer to be empty after reading one record"
        );

        Ok(())
    }

    /// This test verifies that the `PingerSession` respects the `max_in_flight` limit.
    /// It does this by acquiring all available semaphore permits before calling `tick()`,
    /// and then asserting that no new ping task is spawned (by checking that the
    /// sequence number is not incremented).
    #[tokio::test]
    async fn pinger_session_respects_max_in_flight() -> Result<()> {
        // 1. Setup
        tokio::time::pause(); // Pause time to control the interval timer
        let cli = Arc::new(Cli {
            target: "127.0.0.1".parse().unwrap(),
            rate: 1, // 1 pps = 1s interval
            max_in_flight: 1,
            database_addr: "127.0.0.1:7878".to_string(),
        });
        let mut buffer: Vec<u8> = Vec::new();
        let mut session = PingerSession::new(&mut buffer, Arc::clone(&cli))?;

        // 2. Action: Acquire the only available permit. This ensures that when we
        // call `tick()`, the semaphore will be full.
        let _permit = session
            .ping_semaphore
            .clone()
            .try_acquire_owned()
            .expect("Should be able to acquire the only permit");

        // 3. Execution: Advance time and call tick.
        // We advance time just enough to make the interval timer fire.
        tokio::time::advance(Duration::from_millis(1100)).await;

        // Since we are holding the only permit, the `tick()` method should not
        // attempt to create a client or spawn a new ping task. It should complete
        // successfully without doing anything.
        session.tick().await?;

        // 4. Verification: Check that the sequence index was not incremented.
        // This is our proxy for asserting that a ping task was not spawned.
        assert_eq!(
            session.sequence_idx, 0,
            "Sequence index should not be incremented when no permits are available"
        );

        Ok(())
    }
}
