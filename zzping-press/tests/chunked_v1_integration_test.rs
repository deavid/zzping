// This integration test covers basic functionality and a comprehensive scenario.
// More specific tests for edge cases, corruption, etc., are in other files.

use anyhow::Result;
use std::fs::File;
use std::io::Read;
use std::time::Duration;
use zzping_press::{RawDataRecord, chunked_v1};

#[derive(Debug)]
struct ErrorStats {
    drift_violations: Vec<DriftViolation>,
    rtt_violations: Vec<RttViolation>,
    max_individual_drift_ns: i64,
    max_rtt_error_ns: i64,
    total_drift_violations: usize,
    total_rtt_violations: usize,
}

#[derive(Debug)]
struct DriftViolation {
    index: usize,
    cumulative_drift_ns: i64,
    individual_drift_ns: i64,
    original_sent_ns: u64,
    decompressed_sent_ns: u64,
}

#[derive(Debug)]
struct RttViolation {
    index: usize,
    original_rtt_ns: u64,
    decompressed_rtt_ns: u64,
    error_ns: i64,
    tolerance_ns: i64,
}

impl ErrorStats {
    fn new() -> Self {
        Self {
            drift_violations: Vec::new(),
            rtt_violations: Vec::new(),
            max_individual_drift_ns: 0,
            max_rtt_error_ns: 0,
            total_drift_violations: 0,
            total_rtt_violations: 0,
        }
    }

    fn format_duration_ns(ns: i64) -> String {
        let abs_ns = ns.abs();
        let sign = if ns < 0 { "-" } else { "" };

        if abs_ns >= 1_000_000 {
            format!("{}{:.3}ms", sign, abs_ns as f64 / 1_000_000.0)
        } else if abs_ns >= 1_000 {
            format!("{}{:.3}µs", sign, abs_ns as f64 / 1_000.0)
        } else {
            format!("{}{}ns", sign, abs_ns)
        }
    }

    fn print_summary(&self) {
        println!("\n=== ERROR STATISTICS SUMMARY ===");
        println!("Total records analyzed: (will be set by caller)");
        println!("Timestamp violations: {}", self.total_drift_violations);
        println!("RTT violations: {}", self.total_rtt_violations);
        println!(
            "Max individual timestamp error: {}",
            Self::format_duration_ns(self.max_individual_drift_ns)
        );
        println!(
            "Max RTT error: {}",
            Self::format_duration_ns(self.max_rtt_error_ns)
        );

        if !self.drift_violations.is_empty() {
            println!("\n=== DRIFT VIOLATION SAMPLES ===");
            let sample_count = self.drift_violations.len().min(3);
            for (i, violation) in self.drift_violations.iter().take(sample_count).enumerate() {
                println!(
                    "Sample {} - Index {}: Cumulative drift = {}, Individual drift = {}",
                    i + 1,
                    violation.index,
                    Self::format_duration_ns(violation.cumulative_drift_ns),
                    Self::format_duration_ns(violation.individual_drift_ns)
                );
                println!(
                    "  Original sent: {}ns, Decompressed sent: {}ns",
                    violation.original_sent_ns, violation.decompressed_sent_ns
                );
            }
        }

        if !self.rtt_violations.is_empty() {
            println!("\n=== RTT VIOLATION SAMPLES ===");
            let sample_count = self.rtt_violations.len().min(3);
            for (i, violation) in self.rtt_violations.iter().take(sample_count).enumerate() {
                println!(
                    "Sample {} - Index {}: RTT error = {} (tolerance: {})",
                    i + 1,
                    violation.index,
                    Self::format_duration_ns(violation.error_ns),
                    Self::format_duration_ns(violation.tolerance_ns)
                );
                println!(
                    "  Original RTT: {}, Decompressed RTT: {}",
                    Self::format_duration_ns(violation.original_rtt_ns as i64),
                    Self::format_duration_ns(violation.decompressed_rtt_ns as i64)
                );
            }
        }
        println!("=====================================\n");
    }
}

#[test]
fn test_round_trip() -> Result<()> {
    // This test uses a 10-minute real-world dataset to verify the round-trip correctness.
    // 1. Load the fixture file.
    let mut file = File::open("tests/fixtures/ten-minutes.dat")?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;

    // Skip the 16-byte header to get to the records.
    let records_data = &buffer[16..];
    let mut original_records: Vec<RawDataRecord> = records_data
        .chunks_exact(16)
        .map(|chunk| RawDataRecord {
            sent_nanos: u64::from_le_bytes(chunk[0..8].try_into().unwrap()),
            rtt_nanos: u64::from_le_bytes(chunk[8..16].try_into().unwrap()),
        })
        .collect();
    original_records.sort();

    assert_eq!(
        original_records.len(),
        60000,
        "Fixture should have 60000 records"
    );

    // 2. Compress the data.
    let compressed_data = chunked_v1::compress_chunked_v1(&original_records)?;
    println!(
        "Compression ratio: {:.2}x ({} bytes -> {} bytes)",
        buffer.len() as f64 / compressed_data.len() as f64,
        buffer.len(),
        compressed_data.len()
    );

    // 3. Decompress the data.
    let mut decompressed_records = chunked_v1::decompress_chunked_v1(&compressed_data)?;
    decompressed_records.sort();

    // 4. Assert correctness.
    assert_eq!(original_records.len(), decompressed_records.len());

    let mut error_stats = ErrorStats::new();

    // === PROPER TIMING VALIDATION ===

    // 1. End-to-end duration preservation (most important metric)
    let original_duration =
        original_records.last().unwrap().sent_nanos - original_records[0].sent_nanos;
    let decompressed_duration =
        decompressed_records.last().unwrap().sent_nanos - decompressed_records[0].sent_nanos;
    let duration_error_ns = (original_duration as i64 - decompressed_duration as i64).abs();
    const MAX_DURATION_ERROR_NS: i64 = 10_000_000; // 10ms total duration tolerance

    // 2. Individual timestamp precision
    const MAX_INDIVIDUAL_TIMESTAMP_ERROR_NS: i64 = 5_000_000; // 5ms per timestamp

    // 3. Interval accuracy (timing between consecutive pings)
    let mut max_interval_error_ns = 0i64;
    let mut interval_violations = 0;
    const MAX_INTERVAL_ERROR_NS: i64 = 10_000_000; // 10ms per interval

    // Calculate original and decompressed intervals
    for i in 1..original_records.len() {
        let orig_interval = original_records[i].sent_nanos - original_records[i - 1].sent_nanos;
        let decomp_interval =
            decompressed_records[i].sent_nanos - decompressed_records[i - 1].sent_nanos;
        let interval_error = (orig_interval as i64 - decomp_interval as i64).abs();

        max_interval_error_ns = max_interval_error_ns.max(interval_error);
        if interval_error > MAX_INTERVAL_ERROR_NS {
            interval_violations += 1;
        }
    }

    // Collect all errors before failing
    for (i, (original, decompressed)) in original_records
        .iter()
        .zip(decompressed_records.iter())
        .enumerate()
    {
        // Check individual timestamp precision
        let timestamp_error = (original.sent_nanos as i64 - decompressed.sent_nanos as i64).abs();
        error_stats.max_individual_drift_ns =
            error_stats.max_individual_drift_ns.max(timestamp_error);

        // Check for timestamp precision violations
        if timestamp_error > MAX_INDIVIDUAL_TIMESTAMP_ERROR_NS {
            error_stats.total_drift_violations += 1;

            // Sample the first few violations for detailed analysis
            if error_stats.drift_violations.len() < 5 {
                error_stats.drift_violations.push(DriftViolation {
                    index: i,
                    cumulative_drift_ns: timestamp_error, // Reusing field for individual error
                    individual_drift_ns: timestamp_error,
                    original_sent_ns: original.sent_nanos,
                    decompressed_sent_ns: decompressed.sent_nanos,
                });
            }
        }

        // Check RTT tolerance
        if original.rtt_nanos == u64::MAX {
            // Packet loss case
            if decompressed.rtt_nanos != u64::MAX {
                error_stats.total_rtt_violations += 1;
                if error_stats.rtt_violations.len() < 5 {
                    error_stats.rtt_violations.push(RttViolation {
                        index: i,
                        original_rtt_ns: original.rtt_nanos,
                        decompressed_rtt_ns: decompressed.rtt_nanos,
                        error_ns: i64::MAX, // Special marker for packet loss mismatch
                        tolerance_ns: 0,
                    });
                }
            }

            // Check lost packet timing accuracy
            let sent_time_diff =
                (original.sent_nanos as i64 - decompressed.sent_nanos as i64).abs();
            if sent_time_diff > 5_000_000 {
                // 5ms tolerance for lost packets
                error_stats.total_drift_violations += 1;
            }
        } else {
            // Normal RTT case
            if i < 10 {
                println!(
                    "Record {}: Original RTT: {}, Decompressed RTT: {}",
                    i,
                    ErrorStats::format_duration_ns(original.rtt_nanos as i64),
                    ErrorStats::format_duration_ns(decompressed.rtt_nanos as i64)
                );
            }

            let rtt_diff_ns = (original.rtt_nanos as i64 - decompressed.rtt_nanos as i64).abs();
            let tolerance_ns = (500_000).max((original.rtt_nanos as f64 * 0.002).round() as i64);

            error_stats.max_rtt_error_ns = error_stats.max_rtt_error_ns.max(rtt_diff_ns);

            if rtt_diff_ns > tolerance_ns {
                error_stats.total_rtt_violations += 1;

                // Sample the first few RTT violations
                if error_stats.rtt_violations.len() < 5 {
                    error_stats.rtt_violations.push(RttViolation {
                        index: i,
                        original_rtt_ns: original.rtt_nanos,
                        decompressed_rtt_ns: decompressed.rtt_nanos,
                        error_ns: rtt_diff_ns,
                        tolerance_ns,
                    });
                }
            }
        }
    }

    // Print comprehensive error statistics
    println!("\nTotal records analyzed: {}", original_records.len());
    error_stats.print_summary();

    // Print new timing metrics
    println!("\n=== TIMING VALIDATION RESULTS ===");
    println!(
        "End-to-end duration error: {} (limit: {})",
        ErrorStats::format_duration_ns(duration_error_ns),
        ErrorStats::format_duration_ns(MAX_DURATION_ERROR_NS)
    );
    println!(
        "Max individual timestamp error: {} (limit: {})",
        ErrorStats::format_duration_ns(error_stats.max_individual_drift_ns),
        ErrorStats::format_duration_ns(MAX_INDIVIDUAL_TIMESTAMP_ERROR_NS)
    );
    println!(
        "Max interval error: {} (limit: {})",
        ErrorStats::format_duration_ns(max_interval_error_ns),
        ErrorStats::format_duration_ns(MAX_INTERVAL_ERROR_NS)
    );
    println!(
        "Interval violations: {} out of {}",
        interval_violations,
        original_records.len() - 1
    );

    // Final assertions with proper error messages
    if duration_error_ns > MAX_DURATION_ERROR_NS {
        panic!(
            "End-to-end duration error too large: {} (limit: {})",
            ErrorStats::format_duration_ns(duration_error_ns),
            ErrorStats::format_duration_ns(MAX_DURATION_ERROR_NS)
        );
    }

    if error_stats.total_drift_violations > 0 {
        panic!(
            "Found {} individual timestamp violations! Max error: {} (limit: {}). See detailed statistics above.",
            error_stats.total_drift_violations,
            ErrorStats::format_duration_ns(error_stats.max_individual_drift_ns),
            ErrorStats::format_duration_ns(MAX_INDIVIDUAL_TIMESTAMP_ERROR_NS)
        );
    }

    if interval_violations > 0 {
        panic!(
            "Found {} interval violations! Max interval error: {} (limit: {})",
            interval_violations,
            ErrorStats::format_duration_ns(max_interval_error_ns),
            ErrorStats::format_duration_ns(MAX_INTERVAL_ERROR_NS)
        );
    }

    if error_stats.total_rtt_violations > 0 {
        panic!(
            "Found {} RTT accuracy violations! Max RTT error: {}. See detailed statistics above.",
            error_stats.total_rtt_violations,
            ErrorStats::format_duration_ns(error_stats.max_rtt_error_ns)
        );
    }

    println!("✅ All timing precision tests passed!");
    println!(
        "   End-to-end duration preserved within {} tolerance",
        ErrorStats::format_duration_ns(MAX_DURATION_ERROR_NS)
    );
    println!(
        "   Max individual timestamp error: {}",
        ErrorStats::format_duration_ns(error_stats.max_individual_drift_ns)
    );
    println!(
        "   Max RTT error: {}",
        ErrorStats::format_duration_ns(error_stats.max_rtt_error_ns)
    );

    Ok(())
}

#[test]
fn test_comprehensive_scenario() -> Result<()> {
    let mut records = Vec::new();
    let start_time = 1_672_531_200_000_000_000;
    for i in 0..120 {
        // 2 minutes of data
        let rtt_nanos = if i % 10 == 0 {
            u64::MAX // 10% packet loss
        } else {
            Duration::from_millis(20 + (i % 30)).as_nanos() as u64 // Variable RTT
        };
        records.push(RawDataRecord {
            sent_nanos: start_time + (i * 1_000_000_000),
            rtt_nanos,
        });
    }

    let compressed = chunked_v1::compress_chunked_v1(&records)?;
    let decompressed = chunked_v1::decompress_chunked_v1(&compressed)?;

    assert_eq!(records.len(), decompressed.len());
    // A simple check for correctness. More detailed checks are in other test files.
    for (orig, decomp) in records.iter().zip(decompressed.iter()) {
        if orig.rtt_nanos != u64::MAX {
            let rtt_diff_ns = (orig.rtt_nanos as i64 - decomp.rtt_nanos as i64).abs();
            let tolerance_ns = (500_000).max((orig.rtt_nanos as f64 * 0.002).round() as i64);
            assert!(rtt_diff_ns <= tolerance_ns);
        }
    }
    Ok(())
}
