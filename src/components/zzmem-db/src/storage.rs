//! Storage backend for the MemDB Database role.
//!
//! This module provides the core storage functionality for ping results
//! in the Database role. It handles insertion, querying, and maintenance
//! of stored ping data.

use crate::network_messages::{PingResult, StoredPingResult};
use std::collections::HashMap;

/// Storage backend for ping results in Database role.
///
/// This struct manages the in-memory storage of ping results with
/// configurable limits per target.
pub struct StorageBackend {
    /// Storage: target -> list of stored results
    data: HashMap<String, Vec<StoredPingResult>>,
    /// Maximum number of results to keep per target
    max_per_target: usize,
}

impl StorageBackend {
    /// Create a new storage backend with the specified limit per target.
    pub fn new(max_per_target: usize) -> Self {
        Self {
            data: HashMap::new(),
            max_per_target,
        }
    }

    /// Insert a batch of ping results into storage.
    ///
    /// Converts PingResult to StoredPingResult, adds storage timestamp,
    /// and enforces storage limits.
    pub fn insert_batch(&mut self, results: Vec<PingResult>, batch_timestamp_ms: u64) {
        for result in results {
            self.insert_single(result, batch_timestamp_ms);
        }
    }

    /// Insert a single ping result into storage.
    fn insert_single(&mut self, result: PingResult, batch_timestamp_ms: u64) {
        let target = result.target.clone();
        let stored = StoredPingResult {
            target: result.target,
            timestamp_ms: result.timestamp_ms,
            rtt_us: result.rtt_us,
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
    pub fn query_target(&self, target: &str, from_ms: u64, to_ms: u64) -> Vec<StoredPingResult> {
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
    pub fn get_target_stats(&self, target: &str) -> (usize, Option<f64>, f64, Option<u64>) {
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

    /// Get the total number of results across all targets.
    pub fn total_results(&self) -> usize {
        self.data.values().map(|results| results.len()).sum()
    }

    /// Get the number of targets being tracked.
    pub fn target_count(&self) -> usize {
        self.data.len()
    }

    /// Clear all data (for testing or reset).
    pub fn clear(&mut self) {
        self.data.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_result(target: &str, timestamp_ms: u64, rtt_us: Option<u32>) -> PingResult {
        PingResult {
            target: target.to_string(),
            timestamp_ms,
            rtt_us,
        }
    }

    #[test]
    fn test_storage_backend_creation() {
        let backend = StorageBackend::new(100);
        assert_eq!(backend.max_per_target, 100);
        assert_eq!(backend.total_results(), 0);
        assert_eq!(backend.target_count(), 0);
    }

    #[test]
    fn test_insert_single() {
        let mut backend = StorageBackend::new(10);
        let result = create_test_result("8.8.8.8", 1234567890, Some(15000));

        backend.insert_single(result, 1234567900);

        assert_eq!(backend.total_results(), 1);
        assert_eq!(backend.target_count(), 1);

        let stored = backend.data.get("8.8.8.8").unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].target, "8.8.8.8");
        assert_eq!(stored[0].timestamp_ms, 1234567890);
        assert_eq!(stored[0].rtt_us, Some(15000));
        assert_eq!(stored[0].stored_at_ms, 1234567900);
    }

    #[test]
    fn test_insert_batch() {
        let mut backend = StorageBackend::new(10);
        let results = vec![
            create_test_result("8.8.8.8", 1000, Some(10000)),
            create_test_result("8.8.8.8", 2000, Some(12000)),
            create_test_result("1.1.1.1", 1500, Some(8000)),
        ];

        backend.insert_batch(results, 3000);

        assert_eq!(backend.total_results(), 3);
        assert_eq!(backend.target_count(), 2);

        let google_results = backend.data.get("8.8.8.8").unwrap();
        assert_eq!(google_results.len(), 2);

        let cloudflare_results = backend.data.get("1.1.1.1").unwrap();
        assert_eq!(cloudflare_results.len(), 1);
    }

    #[test]
    fn test_storage_limits() {
        let mut backend = StorageBackend::new(2); // Only keep 2 per target

        // Insert 3 results for the same target
        for i in 1..=3 {
            let result = create_test_result("8.8.8.8", 1000 * i as u64, Some(10000 + i * 1000));
            backend.insert_single(result, 1000 * i as u64 + 100);
        }

        let results = backend.data.get("8.8.8.8").unwrap();
        assert_eq!(results.len(), 2); // Should only keep 2 most recent

        // Should keep the 2 most recent (timestamps 2000 and 3000)
        assert_eq!(results[0].timestamp_ms, 2000);
        assert_eq!(results[1].timestamp_ms, 3000);
    }

    #[test]
    fn test_query_target() {
        let mut backend = StorageBackend::new(10);
        let results = vec![
            create_test_result("8.8.8.8", 1000, Some(10000)),
            create_test_result("8.8.8.8", 2000, Some(12000)),
            create_test_result("8.8.8.8", 3000, Some(8000)),
            create_test_result("1.1.1.1", 2500, Some(9000)),
        ];

        backend.insert_batch(results, 4000);

        // Query all results for 8.8.8.8
        let queried = backend.query_target("8.8.8.8", 0, u64::MAX);
        assert_eq!(queried.len(), 3);
        assert_eq!(queried[0].timestamp_ms, 1000);
        assert_eq!(queried[1].timestamp_ms, 2000);
        assert_eq!(queried[2].timestamp_ms, 3000);

        // Query time range
        let range_query = backend.query_target("8.8.8.8", 1500, 2500);
        assert_eq!(range_query.len(), 1);
        assert_eq!(range_query[0].timestamp_ms, 2000);

        // Query non-existent target
        let empty_query = backend.query_target("9.9.9.9", 0, u64::MAX);
        assert_eq!(empty_query.len(), 0);
    }

    #[test]
    fn test_get_target_stats() {
        let mut backend = StorageBackend::new(10);
        let results = vec![
            create_test_result("8.8.8.8", 1000, Some(10000)), // Success
            create_test_result("8.8.8.8", 2000, None),        // Loss
            create_test_result("8.8.8.8", 3000, Some(12000)), // Success
        ];

        backend.insert_batch(results, 4000);

        let (count, avg_rtt, loss_percent, last_seen) = backend.get_target_stats("8.8.8.8");

        assert_eq!(count, 3);
        assert_eq!(avg_rtt, Some(11000.0)); // (10000 + 12000) / 2
        assert_eq!(loss_percent, (1.0 / 3.0) * 100.0); // 1 loss out of 3
        assert_eq!(last_seen, Some(3000));
    }

    #[test]
    fn test_get_target_stats_empty() {
        let backend = StorageBackend::new(10);
        let (count, avg_rtt, loss_percent, last_seen) = backend.get_target_stats("8.8.8.8");

        assert_eq!(count, 0);
        assert_eq!(avg_rtt, None);
        assert_eq!(loss_percent, 0.0);
        assert_eq!(last_seen, None);
    }

    #[test]
    fn test_clear() {
        let mut backend = StorageBackend::new(10);
        let result = create_test_result("8.8.8.8", 1000, Some(10000));
        backend.insert_single(result, 2000);

        assert_eq!(backend.total_results(), 1);
        assert_eq!(backend.target_count(), 1);

        backend.clear();

        assert_eq!(backend.total_results(), 0);
        assert_eq!(backend.target_count(), 0);
    }
}
