//! Manages the state for a single, active connection to the `zzping-database`.

use crate::ping_client::{PingClient, PingResult};
use anyhow::Result;
use log::{debug, error};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::AsyncWrite;
use tokio::sync::{mpsc, Semaphore};
use tokio::time::Interval;
use zzping_lib::protocol::{write_record, RawDataRecord};

/// Manages the state and event loop for a single, ongoing pinging session.
///
/// This struct is generic over a `PingClient`, which allows the underlying
/// pinging mechanism to be swapped out. In production, this will be a client
/// that sends real ICMP packets. In testing, it can be a mock client.
///
/// The `PingerSession` is responsible for:
/// - Maintaining a rate-limited loop via a `tokio::time::Interval`.
/// - Using a semaphore to limit the number of concurrent pings in flight.
/// - Calling the injected `PingClient` to send pings.
/// - Receiving `PingResult`s from an MPSC channel.
/// - Serializing the results into `RawDataRecord`s and writing them to the database stream.
struct PingerSession<W, P>
where
    W: AsyncWrite + Unpin + Send,
    P: PingClient,
{
    /// The underlying network stream to the `zzping-database`.
    db_stream: W,
    /// The shared, injectable ping client.
    ping_client: Arc<P>,
    /// A semaphore to limit the number of pings in flight at any one time.
    ping_semaphore: Arc<Semaphore>,
    /// The sending end of the channel for `PingResult`s. Cloned and given to each ping task.
    tx: mpsc::Sender<PingResult>,
    /// The receiving end of the channel for `PingResult`s.
    rx: mpsc::Receiver<PingResult>,
    /// The sequence number for the next ping. Incremented for each ping sent.
    sequence_idx: u16,
    /// The timer that determines how often pings are sent.
    interval: Interval,
    /// The monotonic timestamp of when this session was created. Used to calculate `sent_nanos`.
    start_time_monotonic: Instant,
}

impl<W, P> PingerSession<W, P>
where
    W: AsyncWrite + Unpin + Send,
    P: PingClient,
{
    /// Creates a new `PingerSession`.
    ///
    /// It also returns the `Sender` half of the internal MPSC channel, which
    /// is useful for tests to inject `PingResult` messages.
    pub fn new(
        db_stream: W,
        ping_client: Arc<P>,
        cli: Arc<crate::Cli>,
    ) -> Result<(Self, mpsc::Sender<PingResult>)> {
        let ping_semaphore = Arc::new(Semaphore::new(cli.max_in_flight));
        let (tx, rx) = mpsc::channel(1024);
        let interval_duration = Duration::from_nanos(1_000_000_000 / cli.rate);
        let interval = tokio::time::interval(interval_duration);
        let start_time_monotonic = Instant::now();

        let session = Self {
            db_stream,
            ping_client,
            ping_semaphore,
            tx: tx.clone(),
            rx,
            sequence_idx: 0,
            interval,
            start_time_monotonic,
        };

        Ok((session, tx))
    }

    /// Performs a single iteration of the session's event loop.
    ///
    /// This method waits for one of two events:
    /// 1. The interval timer ticks, triggering a new ping to be sent.
    /// 2. A ping result is received from a completed ping task.
    ///
    /// It returns `Ok(())` to indicate the session should continue, or an `Err`
    /// if a fatal error occurs (e.g., the connection to the database is lost).
    pub async fn tick(&mut self) -> Result<()> {
        tokio::select! {
            _ = self.interval.tick() => {
                if let Ok(permit) = self.ping_semaphore.clone().try_acquire_owned() {
                    // A permit is available, so we can send a ping.
                    self.ping_client.ping(
                        self.sequence_idx,
                        self.tx.clone(),
                        permit,
                        self.start_time_monotonic,
                    ).await;
                    self.sequence_idx = self.sequence_idx.wrapping_add(1);
                } else {
                    debug!("Max in-flight pings reached, skipping this tick.");
                }
            }
            Some(ping_result) = self.rx.recv() => {
                // A ping result has been received from a ping task.
                let record = RawDataRecord {
                    sent_nanos: ping_result.sent_nanos,
                    rtt_nanos: ping_result.rtt.map_or(u64::MAX, |rtt| rtt.as_nanos() as u64),
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
/// This function contains the main loop for a connected session. It creates a
/// `PingerSession` and then calls `tick()` on it repeatedly until the
/// connection is closed or an error occurs.
///
/// This function is generic over the `PingClient` implementation, allowing it
/// to be used with both the real `PingSurgeClient` and the `PingMockClient`.
///
/// # Arguments
/// * `db_stream` - An async writer, typically the TCP stream to the `zzping-database`.
/// * `ping_client` - The shared ping client implementation.
/// * `cli` - The parsed command-line arguments.
pub async fn handle_connection<W, P>(
    db_stream: W,
    ping_client: Arc<P>,
    cli: Arc<crate::Cli>,
) -> Result<()>
where
    W: AsyncWrite + Unpin + Send,
    P: PingClient,
{
    let (mut session, _) = PingerSession::new(db_stream, ping_client, cli)?;

    loop {
        if let Err(e) = session.tick().await {
            // The tick method returns an error if the session should be terminated.
            // We'll log it and then the function will exit, causing the main loop
            // in `main.rs` to attempt a reconnection.
            error!("Pinging session ended with error: {e}");
            return Err(e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ping_mock_client::PingMockClient, Cli};
    use zzping_lib::protocol::read_record;

    fn common_test_setup(
    ) -> (Vec<u8>, Arc<PingMockClient>, Arc<Cli>, mpsc::Sender<PingResult>) {
        let buffer = Vec::new();
        let ping_client = Arc::new(PingMockClient::new());
        let cli = Arc::new(Cli {
            target: "127.0.0.1".parse().unwrap(),
            rate: 10,
            max_in_flight: 4,
            database_addr: "127.0.0.1:7878".to_string(),
        });
        // We need a sender to inject messages, but the session itself in these
        // tests won't be using the real one.
        let (tx, _) = mpsc::channel(1);
        (buffer, ping_client, cli, tx)
    }

    #[tokio::test]
    async fn test_session_writes_record_on_response() -> Result<()> {
        // 1. Setup
        let (mut buffer, ping_client, cli, _) = common_test_setup();
        let (mut session, tx_to_session) =
            PingerSession::new(&mut buffer, ping_client, cli)?;

        // 2. Action: Manually send a PingResult to the session's internal channel.
        let test_rtt = Duration::from_millis(50);
        let sent_nanos = 12345;
        tx_to_session
            .send(PingResult {
                sent_nanos,
                rtt: Some(test_rtt),
            })
            .await?;

        // 3. Execution: Call `tick()` to process the waiting message.
        session.tick().await?;

        // 4. Verification: Check that the correct data was written to the buffer.
        let mut cursor = std::io::Cursor::new(&buffer);
        let record = read_record(&mut cursor)
            .await?
            .expect("Buffer should contain a record");

        assert_eq!(record.sent_nanos, sent_nanos);
        assert_eq!(record.rtt_nanos, test_rtt.as_nanos() as u64);

        Ok(())
    }

    #[tokio::test]
    async fn test_session_respects_max_in_flight() -> Result<()> {
        // 1. Setup
        tokio::time::pause();
        let (buffer, ping_client, cli, _) = common_test_setup();
        let (mut session, _) =
            PingerSession::new(buffer, Arc::clone(&ping_client), Arc::clone(&cli))?;

        // 2. Action: Acquire all available permits.
        let permit1 = session.ping_semaphore.clone().try_acquire_owned()?;
        let permit2 = session.ping_semaphore.clone().try_acquire_owned()?;
        let permit3 = session.ping_semaphore.clone().try_acquire_owned()?;
        let permit4 = session.ping_semaphore.clone().try_acquire_owned()?;
        assert!(session.ping_semaphore.clone().try_acquire_owned().is_err());

        // 3. Execution: Advance time and call tick.
        tokio::time::advance(Duration::from_secs(1)).await;
        session.tick().await?;

        // 4. Verification: Sequence index should not be incremented.
        assert_eq!(session.sequence_idx, 0);

        // Release permits to avoid test deadlocks
        drop(permit1);
        drop(permit2);
        drop(permit3);
        drop(permit4);

        Ok(())
    }
}
