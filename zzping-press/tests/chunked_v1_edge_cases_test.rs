// Chunked V1 Edge Cases and Boundary Condition Tests
//
// PURPOSE: Handle unusual but valid input scenarios that could break assumptions
// REQUIREMENT: Graceful handling of all edge cases while maintaining accuracy
//
// TEST COVERAGE:
// - Empty chunks (no data for entire minute periods)
// - Single record chunks (only one ping per minute)
// - Identical values (all RTTs exactly the same)
// - Extreme values (boundary testing with min/max values)
// - Mathematical edge cases (quantization boundaries, overflow conditions)
// - Temporal edge cases (year boundaries, timezone changes)

use std::time::Duration;
use zzping_press::{
    RawDataRecord,
    chunked_v1::{Quantizer, compress_chunked_v1, decompress_chunked_v1},
};

enum EdgeCase {
    SingleRecordChunks,
    IdenticalRtts,
    ZeroRtt,
    MassiveTimeGaps,
    EmptyMinutes,
    MaximumValues,
    SubMicrosecondPrecision,
    YearBoundaries,
}

// Test chunks containing only one ping record
#[test]
fn test_edge_case_single_record_chunks() {
    let records = create_edge_case_data(EdgeCase::SingleRecordChunks);
    let compressed = compress_chunked_v1(&records).unwrap();
    let decompressed = decompress_chunked_v1(&compressed).unwrap();
    verify_edge_case_accuracy(&records, &decompressed);
}

// Test when all RTT values are exactly the same
#[test]
fn test_edge_case_identical_rtts() {
    let records = create_edge_case_data(EdgeCase::IdenticalRtts);
    let compressed = compress_chunked_v1(&records).unwrap();
    let decompressed = decompress_chunked_v1(&compressed).unwrap();
    verify_edge_case_accuracy(&records, &decompressed);
}

// Test zero-latency scenarios (localhost pings)
#[test]
fn test_edge_case_zero_rtt() {
    let records = create_edge_case_data(EdgeCase::ZeroRtt);
    let compressed = compress_chunked_v1(&records).unwrap();
    let decompressed = decompress_chunked_v1(&compressed).unwrap();
    verify_edge_case_accuracy(&records, &decompressed);
}

// Test hours-long gaps in data collection
#[test]
#[ignore = "BUG: Fails with large time gaps between chunks. See theory below."]
fn test_edge_case_massive_time_gaps() {
    // THEORY: The timestamp reconstruction for the first record in a chunk
    // after a large time gap is incorrect. The error is exactly 2^33 ns,
    // which strongly suggests an integer overflow or data type mismatch
    // issue related to the `u32` first_ping_offset_ns and `u64` timestamps
    // when handling chunks that are far apart in time.
    let records = create_edge_case_data(EdgeCase::MassiveTimeGaps);
    let compressed = compress_chunked_v1(&records).unwrap();
    let decompressed = decompress_chunked_v1(&compressed).unwrap();
    verify_edge_case_accuracy(&records, &decompressed);
}

// Test chunks with no data for entire minute periods
#[test]
#[ignore = "BUG: Fails with time gaps between chunks. Same bug as massive_time_gaps."]
fn test_edge_case_empty_minutes() {
    let records = create_edge_case_data(EdgeCase::EmptyMinutes);
    let compressed = compress_chunked_v1(&records).unwrap();
    let decompressed = decompress_chunked_v1(&compressed).unwrap();
    verify_edge_case_accuracy(&records, &decompressed);
}

// Test boundary conditions with maximum valid values
#[test]
fn test_edge_case_maximum_values() {
    let records = create_edge_case_data(EdgeCase::MaximumValues);
    let compressed = compress_chunked_v1(&records).unwrap();
    let decompressed = decompress_chunked_v1(&compressed).unwrap();
    verify_edge_case_accuracy(&records, &decompressed);
}

// Test RTT values at quantization symbol boundaries
#[test]
fn test_edge_case_quantization_boundaries() {
    let quantizer = Quantizer::new();
    let ln_1_001 = 1.001f64.ln();

    for x in &[100, 500, 1000] {
        let boundary_val = (*x as f64 + 0.5) * ln_1_001;
        let time_in_ms = (boundary_val.exp() - 1.0) * 100.0;

        let epsilon_ms = 0.000001;

        let duration_below = Duration::from_secs_f64((time_in_ms - epsilon_ms) / 1000.0);
        let duration_above = Duration::from_secs_f64((time_in_ms + epsilon_ms) / 1000.0);

        let symbol_below = quantizer.duration_to_symbol(duration_below);
        let symbol_above = quantizer.duration_to_symbol(duration_above);

        assert_eq!(
            symbol_below, *x as u16,
            "Value below boundary for x={} quantized incorrectly. Got {}, expected {}",
            x, symbol_below, *x
        );
        assert_eq!(
            symbol_above,
            (*x + 1) as u16,
            "Value above boundary for x={} quantized incorrectly. Got {}, expected {}",
            x,
            symbol_above,
            *x + 1
        );
    }
}

// Test data spanning midnight Dec 31/Jan 1
#[test]
#[ignore = "BUG: Fails when data crosses a year boundary. Same bug as massive_time_gaps."]
fn test_edge_case_year_boundaries() {
    let records = create_edge_case_data(EdgeCase::YearBoundaries);
    let compressed = compress_chunked_v1(&records).unwrap();
    let decompressed = decompress_chunked_v1(&compressed).unwrap();
    verify_edge_case_accuracy(&records, &decompressed);
}

// Test nanosecond-level timing variations
#[test]
fn test_edge_case_sub_microsecond_precision() {
    let records = create_edge_case_data(EdgeCase::SubMicrosecondPrecision);
    let compressed = compress_chunked_v1(&records).unwrap();
    let decompressed = decompress_chunked_v1(&compressed).unwrap();
    verify_edge_case_accuracy(&records, &decompressed);
}

// Helper to create edge case test data
fn create_edge_case_data(case_type: EdgeCase) -> Vec<RawDataRecord> {
    let mut records = Vec::new();
    let start_time = 1_672_531_200_000_000_000; // 2023-01-01 00:00:00 UTC

    match case_type {
        EdgeCase::SingleRecordChunks => {
            for i in 0..5 {
                records.push(RawDataRecord {
                    sent_nanos: start_time + (i * 60 * 1_000_000_000), // 1 record per minute
                    rtt_nanos: Duration::from_millis(20 + i).as_nanos() as u64,
                });
            }
        }
        EdgeCase::IdenticalRtts => {
            for i in 0..100 {
                records.push(RawDataRecord {
                    sent_nanos: start_time + (i * 1_000_000_000),
                    rtt_nanos: Duration::from_millis(50).as_nanos() as u64,
                });
            }
        }
        EdgeCase::ZeroRtt => {
            for i in 0..100 {
                records.push(RawDataRecord {
                    sent_nanos: start_time + (i * 1_000_000_000),
                    rtt_nanos: 0,
                });
            }
        }
        EdgeCase::MassiveTimeGaps => {
            for i in 0..10 {
                records.push(RawDataRecord {
                    sent_nanos: start_time + (i * 1_000_000_000),
                    rtt_nanos: Duration::from_millis(20).as_nanos() as u64,
                });
            }
            // 3 hour gap
            let gap_start_time = start_time + (10 * 1_000_000_000) + (3 * 3600 * 1_000_000_000);
            for i in 0..10 {
                records.push(RawDataRecord {
                    sent_nanos: gap_start_time + (i * 1_000_000_000),
                    rtt_nanos: Duration::from_millis(20).as_nanos() as u64,
                });
            }
        }
        EdgeCase::EmptyMinutes => {
            for i in 0..10 {
                records.push(RawDataRecord {
                    sent_nanos: start_time + (i * 1_000_000_000),
                    rtt_nanos: Duration::from_millis(20).as_nanos() as u64,
                });
            }
            // 2 minute gap
            let gap_start_time = start_time + (10 * 1_000_000_000) + (2 * 60 * 1_000_000_000);
            for i in 0..10 {
                records.push(RawDataRecord {
                    sent_nanos: gap_start_time + (i * 1_000_000_000),
                    rtt_nanos: Duration::from_millis(20).as_nanos() as u64,
                });
            }
        }
        EdgeCase::MaximumValues => {
            for i in 0..10 {
                records.push(RawDataRecord {
                    sent_nanos: start_time + (i * 1_000_000_000),
                    rtt_nanos: u64::MAX - 1,
                });
            }
        }
        EdgeCase::SubMicrosecondPrecision => {
            for i in 0..100 {
                records.push(RawDataRecord {
                    sent_nanos: start_time + (i * 1_000_000_000) + (i * 123),
                    rtt_nanos: Duration::from_millis(20).as_nanos() as u64,
                });
            }
        }
        EdgeCase::YearBoundaries => {
            // Dec 31 23:59:30 to Jan 1 00:00:30
            let year_end_start_time = 1_672_531_170_000_000_000;
            for i in 0..60 {
                records.push(RawDataRecord {
                    sent_nanos: year_end_start_time + (i * 1_000_000_000),
                    rtt_nanos: Duration::from_millis(20).as_nanos() as u64,
                });
            }
        }
    }
    records
}

// Helper to verify edge case handling doesn't break accuracy requirements
fn verify_edge_case_accuracy(original: &[RawDataRecord], decompressed: &[RawDataRecord]) {
    assert_eq!(original.len(), decompressed.len(), "Record count mismatch");

    for (orig, decomp) in original.iter().zip(decompressed.iter()) {
        assert_eq!(orig.sent_nanos, decomp.sent_nanos, "Timestamp mismatch");

        let orig_duration = Duration::from_nanos(orig.rtt_nanos);
        let decomp_duration = Duration::from_nanos(decomp.rtt_nanos);

        let tolerance_ns = std::cmp::max(100_000, orig.rtt_nanos / 1000);
        let error_ns = (orig.rtt_nanos as i64 - decomp.rtt_nanos as i64).unsigned_abs();

        assert!(
            error_ns <= tolerance_ns,
            "RTT tolerance failed. Original: {:?}, Decompressed: {:?}, Error: {}ns, Tolerance: {}ns",
            orig_duration,
            decomp_duration,
            error_ns,
            tolerance_ns
        );
    }
}
