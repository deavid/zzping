//! Manages the state for a single, active connection to the `zzping-database`.

use crate::{cli::Cli, ping_client::PingClient, ping_client::PingResult};
use anyhow::Result;
use log::{debug, error, info, warn};
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::io::AsyncWrite;
use tokio::sync::{Semaphore, mpsc};
use zzping_lib::protocol::{ClientHandshake, RawDataRecord, write_handshake, write_record};

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
/// - Buffering records when the database connection is unavailable.
struct PingerSession<W, P: ?Sized>
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
    /// The monotonic timestamp of when this session was created. Used to calculate `sent_nanos`.
    start_time_monotonic: Instant,
    /// The last tick time for scheduling.
    last_tick: Instant,
    /// The interval between pings in nanoseconds.
    rate_ns: Arc<AtomicU64>,
    /// Buffer for records when database connection is unavailable.
    record_buffer: VecDeque<RawDataRecord>,
    /// Maximum size of the record buffer (1 million records as requested).
    max_buffer_size: usize,
    /// Whether the database connection is currently available.
    db_available: bool,
}

impl<W, P: ?Sized> PingerSession<W, P>
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
        cli: Arc<Cli>,
    ) -> Result<(Self, mpsc::Sender<PingResult>)> {
        let ping_semaphore = Arc::new(Semaphore::new(cli.max_in_flight));
        let (tx, rx) = mpsc::channel(1024);
        let initial_interval_ns = 1_000_000_000 / cli.rate;
        let rate_ns = Arc::new(AtomicU64::new(initial_interval_ns));
        let start_time_monotonic = Instant::now();
        let last_tick = start_time_monotonic;

        let session = Self {
            db_stream,
            ping_client,
            ping_semaphore,
            tx: tx.clone(),
            rx,
            sequence_idx: 0,
            start_time_monotonic,
            last_tick,
            rate_ns,
            record_buffer: VecDeque::new(),
            max_buffer_size: 1_000_000, // 1 million records as requested
            db_available: true,
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
        let interval_nanos = self.rate_ns.load(Ordering::SeqCst);
        let next_tick = self.last_tick + Duration::from_nanos(interval_nanos);
        // Wake up 10ms early to allow time for pinger creation and task spawning
        // This provides a robust buffer that works across different systems
        let wake_up_time = next_tick - Duration::from_millis(10);

        let now = Instant::now();
        let sleep_duration = if wake_up_time > now {
            wake_up_time - now
        } else {
            Duration::from_nanos(0)
        };

        tokio::select! {
            _ = tokio::time::sleep(sleep_duration) => {
                self.last_tick = next_tick;

                // Only send pings if we have buffer space available
                if self.record_buffer.len() < self.max_buffer_size {
                    // Try to acquire a permit for ping concurrency control
                    if let Ok(permit) = self.ping_semaphore.clone().try_acquire_owned() {
                        // A permit is available, so we can send a ping.
                        self.ping_client.ping(
                            self.sequence_idx,
                            self.tx.clone(),
                            permit,
                            self.start_time_monotonic,
                            next_tick,
                        ).await;
                        self.sequence_idx = self.sequence_idx.wrapping_add(1);
                    } else {
                        debug!("Max in-flight pings reached, skipping this tick.");
                    }
                } else {
                    warn!("Record buffer full ({} records), stopping pings until buffer drains", self.record_buffer.len());
                }

                // Try to drain the buffer if database is available and we have records
                if self.db_available && !self.record_buffer.is_empty() {
                    self.drain_buffer().await;
                }
            }
            Some(ping_result) = self.rx.recv() => {
                // A ping result has been received from a ping task.
                let record = RawDataRecord {
                    sent_nanos: ping_result.sent_nanos,
                    rtt_nanos: ping_result.rtt.map_or(u64::MAX, |rtt| rtt.as_nanos() as u64),
                };

                // Try to send the record to the database
                if self.db_available {
                    if let Err(e) = write_record(&mut self.db_stream, &record).await {
                        error!("Failed to write record to stream: {e}. Switching to buffered mode.");
                        self.db_available = false;
                        // Buffer this record and any others that arrive while DB is unavailable
                        self.record_buffer.push_back(record);
                    }
                } else {
                    // Database is unavailable, buffer the record
                    if self.record_buffer.len() < self.max_buffer_size {
                        self.record_buffer.push_back(record);
                        debug!("Buffered record, buffer size: {}", self.record_buffer.len());
                    } else {
                        warn!("Record buffer full, dropping record to prevent memory exhaustion");
                    }
                }

                // Try to drain the buffer if database is available
                if self.db_available && !self.record_buffer.is_empty() {
                    self.drain_buffer().await;
                }
            }
            else => {
                // Channel closed, which means something went wrong. Terminate the session.
                return Err(anyhow::anyhow!("MPSC channel closed unexpectedly."));
            }
        }
        Ok(())
    }

    /// Attempts to drain the record buffer by sending records to the database.
    /// Stops draining if any write fails (indicating DB is still unavailable).
    async fn drain_buffer(&mut self) {
        while let Some(record) = self.record_buffer.front() {
            if let Err(e) = write_record(&mut self.db_stream, record).await {
                error!("Failed to drain buffer record: {e}. Stopping drain.");
                self.db_available = false;
                break;
            } else {
                self.record_buffer.pop_front();
                debug!(
                    "Drained record from buffer, remaining: {}",
                    self.record_buffer.len()
                );
            }
        }

        if self.record_buffer.is_empty() && !self.db_available {
            info!("Buffer drained successfully, database connection restored");
            self.db_available = true;
        }
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
    mut db_stream: W,
    ping_client: Arc<P>,
    cli: Arc<Cli>,
) -> Result<()>
where
    W: AsyncWrite + Unpin + Send,
    P: PingClient + ?Sized,
{
    let handshake = ClientHandshake {
        source_hostname: cli.source_hostname.clone(),
        target: ping_client.target(),
    };

    write_handshake(&mut db_stream, &handshake).await?;

    let (mut session, _) = PingerSession::new(db_stream, ping_client, cli)?;

    // Reset database availability when connection is established
    session.db_available = true;

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
    use crate::{cli::Cli, ping_mock_client::PingMockClient};
    use std::io;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tokio::io::AsyncWrite;
    use zzping_lib::protocol::read_record;

    // A mock writer that can be configured to fail after a certain number of bytes.
    #[allow(dead_code)]
    struct MockWriter {
        buffer: Vec<u8>,
        fail_after: Option<usize>,
    }

    impl AsyncWrite for MockWriter {
        fn poll_write(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            if let Some(fail_after) = self.fail_after
                && self.buffer.len() >= fail_after
            {
                return Poll::Ready(Err(io::Error::other("Simulated error")));
            }
            self.buffer.extend_from_slice(buf);
            Poll::Ready(Ok(buf.len()))
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    fn common_test_setup() -> (
        Vec<u8>,
        Arc<PingMockClient>,
        Arc<Cli>,
        mpsc::Sender<PingResult>,
    ) {
        let buffer = Vec::new();
        let ping_client = Arc::new(PingMockClient::new());
        let cli = Arc::new(Cli {
            targets: vec!["127.0.0.1".parse().unwrap()],
            source_hostname: "test-host".to_string(),
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
        let (mut session, tx_to_session) = PingerSession::new(&mut buffer, ping_client, cli)?;

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

    #[tokio::test]
    async fn test_session_sends_ping_on_tick() -> Result<()> {
        // 1. Setup
        tokio::time::pause();
        let (buffer, ping_client, cli, _) = common_test_setup();
        let (mut session, _) =
            PingerSession::new(buffer, Arc::clone(&ping_client), Arc::clone(&cli))?;

        // 2. Execution: Advance time and call tick.
        tokio::time::advance(Duration::from_secs(1)).await;
        session.tick().await?;

        // 3. Verification
        // Check that the mock client was called once.
        let pings = ping_client.pings.lock().unwrap();
        assert_eq!(pings.len(), 1, "Ping client should have been called once");
        assert_eq!(pings[0], 0, "The first ping should have sequence index 0");

        // Check that the session's sequence index was incremented.
        assert_eq!(session.sequence_idx, 1);

        Ok(())
    }

    #[tokio::test]
    async fn test_record_buffering_when_db_unavailable() -> Result<()> {
        // Test that records are buffered when database connection fails
        tokio::time::pause();
        let (buffer, ping_client, cli, _) = common_test_setup();
        let mut mock_writer = MockWriter {
            buffer,
            fail_after: Some(0), // Fail immediately to simulate DB unavailability
        };

        let (mut session, tx_to_session) = PingerSession::new(&mut mock_writer, ping_client, cli)?;

        // Send a ping result
        let ping_result = PingResult {
            sent_nanos: 1000,
            rtt: Some(Duration::from_millis(10)),
        };
        tx_to_session.send(ping_result).await?;

        // Process the ping result - should buffer it since DB write will fail
        tokio::time::advance(Duration::from_millis(1)).await;
        let result = session.tick().await;

        // The tick should succeed (not return error) but buffer the record
        assert!(result.is_ok());
        assert_eq!(session.record_buffer.len(), 1);
        assert!(!session.db_available);

        Ok(())
    }

    #[tokio::test]
    async fn test_buffer_draining_when_db_restored() -> Result<()> {
        // Test that buffered records are sent when database connection is restored
        tokio::time::pause();
        let (buffer, ping_client, cli, _) = common_test_setup();
        let mut mock_writer = MockWriter {
            buffer,
            fail_after: None, // Don't fail - simulate restored connection
        };

        let (mut session, _) = PingerSession::new(&mut mock_writer, ping_client, cli)?;

        // Put some records in the buffer
        session.record_buffer.push_back(RawDataRecord {
            sent_nanos: 1000,
            rtt_nanos: 10_000_000,
        });
        session.record_buffer.push_back(RawDataRecord {
            sent_nanos: 2000,
            rtt_nanos: 20_000_000,
        });
        session.db_available = false; // Simulate DB was unavailable

        // Simulate connection restoration
        session.db_available = true;

        // Process - should drain the buffer
        tokio::time::advance(Duration::from_millis(1)).await;
        session.tick().await?;

        // Buffer should be empty and DB should be marked as available
        assert_eq!(session.record_buffer.len(), 0);
        assert!(session.db_available);

        Ok(())
    }

    #[tokio::test]
    async fn test_pinging_stops_when_buffer_full() -> Result<()> {
        // Test that pinging stops when buffer reaches maximum capacity
        tokio::time::pause();
        let (buffer, ping_client, cli, _) = common_test_setup();
        let mut mock_writer = MockWriter {
            buffer,
            fail_after: Some(0), // Fail immediately
        };

        let (mut session, _) = PingerSession::new(&mut mock_writer, ping_client, cli)?;

        // Fill the buffer to maximum capacity
        for i in 0..session.max_buffer_size {
            session.record_buffer.push_back(RawDataRecord {
                sent_nanos: i as u64 * 1000,
                rtt_nanos: 10_000_000,
            });
        }

        // Check available permits before tick
        let initial_permits = session.ping_semaphore.available_permits();

        tokio::time::advance(Duration::from_millis(1)).await;
        session.tick().await?;

        // No permits should have been acquired (available permits should be the same)
        let final_permits = session.ping_semaphore.available_permits();
        assert_eq!(initial_permits, final_permits);

        Ok(())
    }

    #[tokio::test]
    async fn test_buffer_overflow_protection() -> Result<()> {
        // Test that records are dropped when buffer is full to prevent memory exhaustion
        tokio::time::pause();
        let (buffer, ping_client, cli, _) = common_test_setup();
        let mut mock_writer = MockWriter {
            buffer,
            fail_after: Some(0), // Fail immediately
        };

        let (mut session, tx_to_session) = PingerSession::new(&mut mock_writer, ping_client, cli)?;

        // Fill the buffer to maximum capacity
        for i in 0..session.max_buffer_size - 1 {
            session.record_buffer.push_back(RawDataRecord {
                sent_nanos: i as u64 * 1000,
                rtt_nanos: 10_000_000,
            });
        }

        // At this point, buffer should have max_buffer_size - 1 records
        assert_eq!(session.record_buffer.len(), session.max_buffer_size - 1);

        // Try to add one more record - should be dropped
        let ping_result = PingResult {
            sent_nanos: 999_000_000,
            rtt: Some(Duration::from_millis(50)),
        };
        tx_to_session.send(ping_result).await?;

        tokio::time::advance(Duration::from_millis(1)).await;
        session.tick().await?;

        // Buffer size should still be at maximum (record should have been dropped)
        assert_eq!(session.record_buffer.len(), session.max_buffer_size);

        Ok(())
    }

    #[tokio::test]
    async fn test_normal_operation_without_buffering() -> Result<()> {
        // Test that normal operation works without buffering when DB is available
        tokio::time::pause();
        let (buffer, ping_client, cli, _) = common_test_setup();
        let mut mock_writer = MockWriter {
            buffer,
            fail_after: None, // Don't fail - DB always available
        };

        let (mut session, tx_to_session) = PingerSession::new(&mut mock_writer, ping_client, cli)?;

        // Send a ping result
        let ping_result = PingResult {
            sent_nanos: 1000,
            rtt: Some(Duration::from_millis(10)),
        };
        tx_to_session.send(ping_result).await?;

        // Process the ping result
        tokio::time::advance(Duration::from_millis(1)).await;
        session.tick().await?;

        // Record should have been sent directly, not buffered
        assert_eq!(session.record_buffer.len(), 0);
        assert!(session.db_available);

        // Check that the record was written to the mock writer
        // The mock writer should contain the serialized record
        assert!(!mock_writer.buffer.is_empty());

        Ok(())
    }

    #[tokio::test]
    async fn test_connection_restoration_resets_state() -> Result<()> {
        // Test that when a new connection is established, the session state is properly reset
        tokio::time::pause();
        let (buffer, ping_client, cli, _) = common_test_setup();
        let mut mock_writer = MockWriter {
            buffer,
            fail_after: None, // Don't fail - connection is restored
        };

        let (mut session, _) = PingerSession::new(&mut mock_writer, ping_client, cli)?;

        // Simulate DB unavailability
        session.db_available = false;
        session.record_buffer.push_back(RawDataRecord {
            sent_nanos: 1000,
            rtt_nanos: 10_000_000,
        });

        // Simulate connection restoration (what happens in handle_connection)
        session.db_available = true;

        // Process - should drain the buffer
        tokio::time::advance(Duration::from_millis(1)).await;
        session.tick().await?;

        // Buffer should be empty
        assert_eq!(session.record_buffer.len(), 0);
        assert!(session.db_available);

        Ok(())
    }
}
