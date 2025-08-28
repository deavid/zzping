// Chunked V1 Timing Accuracy Tests
//
// PURPOSE: Ensure cumulative timing drift stays within 20ms bounds
// REQUIREMENT: Cumulative drift ≤ 20ms across entire dataset
//
// TEST COVERAGE:
// - Constant Rate Data: Perfect intervals (should have 0 drift with new format)
// - Near-Constant Rate: Small variations, minute boundary testing
// - Cross-chunk behavior: Timing accuracy across minute boundaries
// - Long sequences: 24+ hours of data for drift accumulation testing
// - Clock anomalies: Backward time, NTP adjustments, leap seconds

use zzping_press::RawDataRecord;
use zzping_press::chunked_v1::{compress_chunked_v1, decompress_chunked_v1};
use std::time::Duration;

// Helper to generate test data with specific timing patterns
fn generate_test_data(num_records: usize, interval_pattern: fn(usize) -> u64) -> Vec<RawDataRecord> {
    let mut records = Vec::with_capacity(num_records);
    let mut current_timestamp_ns = 1_672_531_200_000_000_000; // 2023-01-01 00:00:00 UTC

    for i in 0..num_records {
        records.push(RawDataRecord {
            sent_nanos: current_timestamp_ns,
            // RTT is not the focus of these tests, so use a constant value
            rtt_nanos: Duration::from_millis(20).as_nanos() as u64,
        });
        current_timestamp_ns += interval_pattern(i);
    }
    records
}

// Test perfect constant intervals - should have zero drift with minute boundary format
#[test]
fn test_timing_accuracy_constant_rate() {
    let records = generate_test_data(1000, |_| 1_000_000_000); // 1s constant interval
    let compressed = compress_chunked_v1(&records).expect("Compression failed");
    let decompressed = decompress_chunked_v1(&compressed).expect("Decompression failed");

    assert_eq!(records.len(), decompressed.len(), "Record count mismatch");
    let drift = calculate_cumulative_drift(&records, &decompressed);
    assert_eq!(drift, 0, "Constant rate data should have zero cumulative drift");
}

// Test small variations that should still result in minimal drift
#[test]
fn test_timing_accuracy_near_constant() {
    let records = generate_test_data(1000, |i| {
        if i % 2 == 0 {
            1_000_000_000 + 1_000_000 // +1ms jitter
        } else {
            1_000_000_000 - 1_000_000 // -1ms jitter
        }
    });
    let compressed = compress_chunked_v1(&records).expect("Compression failed");
    let decompressed = decompress_chunked_v1(&compressed).expect("Decompression failed");

    assert_eq!(records.len(), decompressed.len(), "Record count mismatch");
    let violations = find_drift_violations(&records, &decompressed);
    assert!(violations.is_empty(), "Near-constant rate data produced drift violations: {:?}", violations);
}

// Verify minute boundary + offset approach maintains accuracy
#[test]
#[ignore = "TODO: Implement minute boundary testing"]
fn test_timing_accuracy_minute_boundaries() {
    todo!("Test minute boundary alignment and offset accuracy");
}

// Test drift accumulation over extended periods (24+ hours)
#[test]
#[ignore = "TODO: Implement long sequence testing"]
fn test_timing_accuracy_long_sequences() {
    todo!("Test timing accuracy over 24+ hour datasets");
}

// Test handling of clock adjustments, backward time, leap seconds
#[test]
#[ignore = "TODO: Implement clock anomaly testing"]
fn test_timing_accuracy_clock_anomalies() {
    todo!("Test clock skew, NTP adjustments, leap seconds");
}

// Test quantized variable rate mode maintains accuracy
#[test]
fn test_timing_accuracy_variable_rate() {
    // Generate timestamps with irregular intervals
    let records = generate_test_data(1000, |i| 800_000_000 + ((i as u64 * 12345) % 400_000_000));
    let compressed = compress_chunked_v1(&records).expect("Compression failed");
    let decompressed = decompress_chunked_v1(&compressed).expect("Decompression failed");

    assert_eq!(records.len(), decompressed.len(), "Record count mismatch");
    let violations = find_drift_violations(&records, &decompressed);
    assert!(violations.is_empty(), "Variable rate data produced drift violations: {:?}", violations);
}

// Calculate the maximum cumulative timing drift across the dataset
fn calculate_cumulative_drift(original: &[RawDataRecord], decompressed: &[RawDataRecord]) -> i64 {
    let mut max_drift = 0i64;
    original.iter().zip(decompressed.iter()).for_each(|(orig, decomp)| {
        let drift = (orig.sent_nanos as i64) - (decomp.sent_nanos as i64);
        if drift.abs() > max_drift.abs() {
            max_drift = drift;
        }
    });
    max_drift
}

// Find all timing drift violations that exceed the 20ms threshold.
// With the new integer-only format, this should always be empty.
fn find_drift_violations(original: &[RawDataRecord], decompressed: &[RawDataRecord]) -> Vec<(usize, i64)> {
    let mut violations = Vec::new();
    original.iter().zip(decompressed.iter()).enumerate().for_each(|(i, (orig, decomp))| {
        let drift = (orig.sent_nanos as i64) - (decomp.sent_nanos as i64);
        if drift != 0 {
            violations.push((i, drift));
        }
    });
    violations
}
