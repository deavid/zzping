use crate::pinger::ping_task;
use anyhow::Result;
use bytes::BufMut;
use log::{debug, error};
use std::sync::Arc;
use std::time::Duration;
use surge_ping::{Client, Config, PingIdentifier};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Semaphore};
use tokio::time::Instant;
use zzping_common::RawDataRecord;

pub async fn handle_connection(mut stream: TcpStream, cli: Arc<crate::Cli>) -> Result<()> {
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

                let json_data = serde_json::to_vec(&record)?;
                let mut packet = Vec::with_capacity(4 + json_data.len());
                packet.put_u32(json_data.len() as u32);
                packet.extend_from_slice(&json_data);

                if let Err(e) = stream.write_all(&packet).await {
                    error!("Failed to write to stream: {e}. Disconnecting.");
                    return Err(e.into());
                }
            }
            else => {
                // Channel closed, which means something went wrong.
                break;
            }
        }
    }
    Ok(())
}
