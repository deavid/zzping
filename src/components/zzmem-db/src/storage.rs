//! Storage backend for the MemDB Database role.
//!
//! This module provides the core storage functionality for ping results
//! in the Database role. It handles insertion, querying, and maintenance
//! of stored ping data.

use crate::network_messages::StoredPingResult;
use crate::types::PingResult;
use std::collections::HashMap;

/// Storage backend for ping results in Database role.
///
/// This struct manages the in-memory storage of ping results with
/// configurable limits per target.
pub(crate) struct StorageBackend {
    /// Storage: target -> list of stored results
    data: HashMap<String, Vec<StoredPingResult>>,
    /// Maximum number of results to keep per target
    max_per_target: usize,
}

impl StorageBackend {
    /// Create a new storage backend with the specified limit per target.
    pub(crate) fn new(max_per_target: usize) -> Self {
        Self {
            data: HashMap::new(),
            max_per_target,
        }
    }

    /// Insert a batch of ping results into storage.
    ///
    /// Converts PingResult to StoredPingResult, adds storage timestamp,
    /// and enforces storage limits.
    pub(crate) fn insert_batch(&mut self, results: Vec<PingResult>, batch_timestamp_ms: u64) {
        for result in results {
            self.insert_single(result, batch_timestamp_ms);
        }
    }

    /// Insert a single ping result into storage.
    fn insert_single(&mut self, result: PingResult, batch_timestamp_ms: u64) {
        let target = result.target.clone();

        // TODO(v0.3-phase3): This conversion is a temporary shim.
        // This entire StorageBackend will be removed and replaced with a
        // flush-to-zzstorage mechanism.
        let stored = StoredPingResult {
            target: result.target,
            timestamp_ms: result.sent_time_ns / 1_000_000, // ns to ms
            rtt_us: match result.status {
                crate::types::PingStatus::Success(ns) => Some((ns / 1000) as u32), // ns to us
                _ => None,
            },
            stored_at_ms: batch_timestamp_ms,
        };

        // Insert into storage
        self.data.entry(target.clone()).or_default().push(stored);

        // Enforce storage limits
        self.prune_target(&target);
    }

    /// Query ping results for a target within a time range.
    ///
    /// Returns all results for the target that fall within [from_ms, to_ms].
    /// Results are sorted by timestamp (oldest first).
    pub(crate) fn query_target(
        &self,
        target: &str,
        from_ms: u64,
        to_ms: u64,
    ) -> Vec<StoredPingResult> {
        let mut results = self
            .data
            .get(target)
            .map(|results| {
                results
                    .iter()
                    .filter(|r| r.timestamp_ms >= from_ms && r.timestamp_ms <= to_ms)
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        // Sort by timestamp (oldest first)
        results.sort_by_key(|r| r.timestamp_ms);
        results
    }

    /// Get statistics for a target.
    ///
    /// Returns (result_count, avg_rtt_us, packet_loss_percent, last_seen_ms).
    pub(crate) fn get_target_stats(&self, target: &str) -> (usize, Option<f64>, f64, Option<u64>) {
        let results = self.data.get(target).map(|r| r.as_slice()).unwrap_or(&[]);

        let total_count = results.len();
        if total_count == 0 {
            return (0, None, 0.0, None);
        }

        let packet_loss_percent = {
            let lost_packets = results.iter().filter(|r| r.rtt_us.is_none()).count();
            (lost_packets as f64 / total_count as f64) * 100.0
        };

        let successful_rtts: Vec<u32> = results.iter().filter_map(|r| r.rtt_us).collect();
        let avg_rtt_us =
            successful_rtts.iter().sum::<u32>() as f64 / successful_rtts.len().max(1) as f64;

        let last_seen_ms = results.iter().map(|r| r.timestamp_ms).max();

        (
            total_count,
            Some(avg_rtt_us),
            packet_loss_percent,
            last_seen_ms,
        )
    }

    /// Prune old results for a specific target to stay within limits.
    fn prune_target(&mut self, target: &str) {
        if let Some(results) = self.data.get_mut(target)
            && results.len() > self.max_per_target
        {
            // Sort by timestamp (oldest first) and keep only the most recent
            results.sort_by_key(|r| r.timestamp_ms);
            let excess = results.len() - self.max_per_target;
            results.drain(0..excess);
        }
    }
}
