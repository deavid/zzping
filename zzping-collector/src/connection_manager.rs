//! Manages the state for a single, active connection to the `zzping-database`.

use crate::pinger::ping_task;
use anyhow::Result;
use log::{debug, error};
use std::sync::Arc;
use std::time::Duration;
use surge_ping::{Client, Config, PingIdentifier};
use tokio::sync::{mpsc, Semaphore};
use tokio::time::Instant;
use zzping_lib::protocol::{write_record, RawDataRecord};

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
/// * `stream` - The active TCP stream to the `zzping-database`.
/// * `cli` - The parsed command-line arguments.
pub async fn handle_connection<W>(mut stream: W, cli: Arc<crate::Cli>) -> Result<()>
where
    W: tokio::io::AsyncWrite + Unpin + Send,
{
    // Not unit tested: This function is the main integration point for the collector's
    // logic. A unit test would require extensive mocking of the `surge-ping` client
    // and the `TcpStream`. The core protocol logic is tested in `zzping-lib`, and
    // the individual components are kept simple. A full integration test is the
    // most effective way to validate this function's behavior.

    let pinger_config = Config::default();
    let client = Client::new(&pinger_config)?;
    let pinger_ident = PingIdentifier(rand::random());
    let ping_semaphore = Arc::new(Semaphore::new(cli.max_in_flight));

    let (tx, mut rx) = mpsc::channel(1024);

    let mut sequence_idx: u16 = 0;
    let interval_duration = Duration::from_nanos(1_000_000_000 / cli.rate);
    let mut interval = tokio::time::interval(interval_duration);

    let start_time_monotonic = Instant::now();

    loop {
        tokio::select! {
            _ = interval.tick() => {
                if let Ok(permit) = ping_semaphore.clone().try_acquire_owned() {
                    let pinger = client.pinger(cli.target, pinger_ident).await;
                    let tx_clone = tx.clone();
                    tokio::spawn(ping_task(pinger, sequence_idx, tx_clone, permit));
                    sequence_idx = sequence_idx.wrapping_add(1);
                } else {
                    debug!("Max in-flight pings reached, skipping this tick.");
                }
            }
            Some(ping_result) = rx.recv() => {
                let sent_nanos = start_time_monotonic.elapsed().as_nanos() as u64;
                let rtt_nanos = ping_result.rtt.map_or(u64::MAX, |rtt| rtt.as_nanos() as u64);

                let record = RawDataRecord {
                    sent_nanos,
                    rtt_nanos,
                };

                if let Err(e) = write_record(&mut stream, &record).await {
                    error!("Failed to write record to stream: {e}. Disconnecting.");
                    return Err(e);
                }
            }
            else => {
                // Channel closed, which means something went wrong (e.g., all sender tasks panicked).
                break;
            }
        }
    }
    Ok(())
}
