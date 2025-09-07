// NOTE: The contents of this file have been temporarily commented out to allow
// the project to compile during the Chapter 1 refactoring. This component
// will be completely redesigned and reimplemented in Chapter 2.

/*
//! Buffers and periodically submits ping results to the database.
//!
//! This module organizes ping results into per-target buffers and ensures
//! data consistency through acknowledgment (ACK) cursors and retries on
//! desynchronization (DESYNC). It balances memory usage and latency.

use crate::{database_client::SharedDatabaseClient, ping_client::PingResult};
use anyhow::Result;
use log::{error, info, warn};
use std::{collections::HashMap, net::IpAddr, time::Duration};
use tokio::sync::mpsc;
use zzping_proto::zzping::{send_batch_response, RawDataRecord, SendBatchRequest};

/// Buffers ping results and sends them to the database.
///
/// Organizes results by target IP, periodically flushes them, and manages
/// acknowledgment cursors to ensure data consistency.
pub struct BatchSubmitter {
    // NOTE: I do not particularly like that we have client+collector_uuid+token as separate things since this actually looks like a single gRPC entity
    // ... to connect to the database. It feels like something that could be abstracted away and reused for anything in the collector that wants to talk
    // ... to the database.

    // NOTE: The fact that SharedDatabaseClient is an Arc+Mutex makes me wary that this might only allow 1 concurrent request.
    // ... This needs to be analyzed properly.

    /// Shared database client for sending batches.
    client: SharedDatabaseClient,

        /// Unique identifier for this collector instance.
    collector_uuid: String,

    /// Authentication token for database access.
    token: String,

    // NOTE: ping_results_rx, buffers and last_acked_nanos are managing multiple queues at once, making the code more complex.
    // ... There is no requirement to make this a global queue manager AFAIK; and it's very much possible that this can be
    // ... subdivided into a per-target batch submitter. What it is not clear to me is what to do with ping_results_rx
    // ... and the root BatchSubmitter instance itself, as they could be split too, but that seems to create an architecture
    // ... complexity. I also don't really like that we have a HashMap here, but guess it is okay?

    /// Receives ping results from worker tasks.
    ping_results_rx: mpsc::Receiver<PingResult>,

    /// Per-target buffers for storing ping records.
    buffers: HashMap<IpAddr, std::collections::VecDeque<RawDataRecord>>,

    /// Tracks the last acknowledged timestamp for each target.
    last_acked_nanos: HashMap<IpAddr, u64>,
}

impl BatchSubmitter {
    /// Creates a new batch submitter with the given configuration.
    ///
    /// Initializes buffers and acknowledgment tracking for each target.
    pub fn new(
        client: SharedDatabaseClient,
        ping_results_rx: mpsc::Receiver<PingResult>,
        collector_uuid: String,
        token: String,
    ) -> Self {
        Self {
            client,
            ping_results_rx,
            collector_uuid,
            token,
            buffers: HashMap::new(),
            last_acked_nanos: HashMap::new(),
        }
    }

    /// Runs the batch submitter's main event loop.
    ///
    /// Buffers incoming ping results and periodically sends batches to the database.
    /// Ensures memory bounds and handles retries on failures.
    pub async fn run(mut self) -> Result<()> {
        // TODO: This buffer limit should be configurable per instance, especially if we want to do tests that test the behavior.
        const BUFFER_LIMIT: usize = 1_000_000;
        // TODO: This interval needs to be configurable too, same reasons. We want a default value of 10ms for production.
        let mut interval = tokio::time::interval(Duration::from_secs(1));

        loop {
            tokio::select! {
                Some(ping_result) = self.ping_results_rx.recv() => {
                    // TODO: This code needs to be moved to its own method.
                    // TODO: The buffer limit should be per-target, everything should be per-target. In fact the only job we need to do
                    // ... here is to choose the correct key for the hashmap, and feed it to another method to continue.
                    // ... Assuming we move buffers+last_acked_nanos into their own struct like TargetBatchSubmitter and this BatchSubmitter
                    // ... holds the HashMap<IpAddr, TargetBatchSubmitter>.
                    let total_buffered: usize = self.buffers.values().map(|v| v.len()).sum();
                    if total_buffered >= BUFFER_LIMIT
                        && let Some((target, buffer)) = self.buffers.iter_mut().max_by_key(|(_, v)| v.len()) {
                            warn!("Buffer limit reached. Dropping oldest record for target {target}");
                            buffer.pop_front();
                        }

                    let record = RawDataRecord {
                        sent_nanos: ping_result.sent_nanos,
                        rtt_nanos: ping_result.rtt.map_or(u64::MAX, |rtt| rtt.as_nanos() as u64),
                    };
                    self.buffers.entry(ping_result.target).or_default().push_back(record);
                    // FIXME: If we are swapping an old collector with a new one, it seems highly likely that the last pings from the old collector
                    // ... would get stuck in the VecDeque buffer and/or the mpsc channel, and not reach the other collector via the database.
                    // ... This is something that must be analyzed.
                }
                _ = interval.tick() => {
                    self.send_batches().await;
                }
            }
            // FIXME: This loop has no way to end. Some control to gracefully stop it would be nice.
            // ... For example, if recv returns None, all we need is to ensure everything is sent (send_batches) and then we can close.
            // ... This should allow this function to be unit-tested.
        }
    }

    /// Sends buffered batches to the database for all targets.
    ///
    /// Implements the ACK/DESYNC protocol to ensure eventual consistency.
    /// Retries on desyncs and logs errors for later retries.
    async fn send_batches(&mut self) {
        // TODO: This for loop would likely be pushed up, to the parent struct that would be handling the different targets, and the
        // ... remaining of the method, to the inner-per target struct.
        for (target, buffer) in self.buffers.iter_mut() {
            if buffer.is_empty() {
                continue;
            }

            let mut needs_immediate_retry = true;
            // FIXME: This needs_immediate_retry can cause a long loop, that could starve the other operations such as receiving
            // ... from the RX channel, or sending other data. We should be fine waiting for the next send_batches iteration for DESYNC.
            while needs_immediate_retry {
                needs_immediate_retry = false;

                if buffer.is_empty() {
                    break;
                }

                // FIXME: records_to_send tries to send everything on the buffer every time, when in reality it should try to send
                // ... whatever is above sent_nanos. We would probably benefit from a BTree here so that we could iterate from a
                // ... particular timestamp.
                let records_to_send: Vec<_> = buffer.iter().cloned().collect();
                let last_acked = *self.last_acked_nanos.get(target).unwrap_or(&0);

                // NOTE: This dance of request crafting, wrapping, metadata insertion, locking and sending feels that it can be at
                // ... least partially moved into the trait DatabaseClient.
                let mut request = tonic::Request::new(SendBatchRequest {
                    collector_uuid: self.collector_uuid.clone(),
                    target_ip: target.to_string(),
                    records: records_to_send,
                    collector_believes_last_acked_nanos: last_acked,
                });
                request.metadata_mut().insert(
                    "authorization",
                    format!("Bearer {}", self.token).parse().unwrap(),
                );
                // FIXME: This seems that prevents other from sending to the DB concurrently
                let client = self.client.lock().await;
                // NOTE: To research: This seems to only send successfully, completely replied pings. We need to figure a, probably different
                // .. interface or something, of the pings "in flight". Which is specially interesting for the real-time part of the apps, knowing
                // .. that the last X attempts none has returned so far, that we haven't heard back yet but we're definitely pinging. This is something
                // .. that probably requires a thought and a design change - maybe by also sending the "ping attempts" to the DB, but unclear what exactly.
                match client.send_batch(request).await {
                    Ok(response) => {
                        match send_batch_response::Status::try_from(
                            response.status,
                        ) {
                            Ok(send_batch_response::Status::Ok) => {
                                let new_acked = response.database_confirms_last_acked_nanos;
                                // FIXME: This info! will show up a lot, very often. We need to find ways of giving overall status information every minute instead.
                                // .. also, this lacks important stuff like how many pings were sent. For an untrained eye, this might look like a single ping, but
                                // .. it may contain a lot of them. We should communicate properly.
                                info!(
                                    "Batch for target {target} sent successfully. New acked_nanos: {new_acked}"
                                );
                                self.last_acked_nanos.insert(*target, new_acked);
                                // FIXME: This removes/clears the buffer. This is wrong because it only says what pings are in-memory on the database, not in disk.
                                // .. We need to keep a cursor for what is in DB memory and does not need to be re-sent, but we cannot clear the buffer because the
                                // .. database might restart and lose the last minutes of data that were not committed to disk. Alternatively, we can make the DB also
                                // .. include in the answer what was the last fsync'ed data to disk; but for that the DB also needs to do a proper fsync - which it doesn't
                                // .. and keep count of the last sent_nanos that was stored and fsync'ed.
                                // .. If that isn't possible, the plan is to just store on the collector 1 million pings and remove the older ones as we go.
                                buffer.retain(|r| r.sent_nanos > new_acked);
                            }
                            Ok(send_batch_response::Status::Desync) => {
                                let new_acked = response.database_confirms_last_acked_nanos;
                                warn!(
                                    "Received DESYNC for target {target}. DB confirms acked_nanos: {new_acked}. Rewinding buffer."
                                );
                                self.last_acked_nanos.insert(*target, new_acked);
                                // FIXME: Here we see the bug. The database likely is asking us to rewind the buffer but we already cleared it. So now doing this retain,
                                // .. most likely is going to do nothing at all.
                                buffer.retain(|r| r.sent_nanos > new_acked);
                                // NOTE: As noted above, this needs_immediate_retry seems problematic.
                                needs_immediate_retry = true;
                            }
                            Err(_) => {
                                error!(
                                    "Unknown status for target {target} in SendBatchResponse: {}",
                                    response.status
                                );
                            }
                        }
                    }
                    Err(e) => {
                        // FIXME: We need to say that it will be retried **later**.
                        error!(
                            "send_batch RPC for target {target} failed: {e}. Data will be retried."
                        );
                        break;
                    }
                }
            }
        }
    }
}
*/
