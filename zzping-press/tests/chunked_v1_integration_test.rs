// TODO: This integration test covers basic functionality but needs expansion according to test plan
//
// MISSING CRITICAL TESTS:
// 1. Edge cases: empty chunks, single record chunks, all packet loss
// 2. Timing anomalies: clock skew, leap seconds, huge gaps
// 3. RTT edge cases: zero RTT, extreme outliers, identical values
// 4. Statistical validation: model accuracy, entropy efficiency
// 5. Performance: large datasets, memory usage, speed benchmarks
// 6. Format compliance: header structure, byte ordering, size validation
// 7. Cross-chunk behavior: timing accuracy across minute boundaries
//
// NEEDED TEST FILES (from test plan):
// - chunked_v1_quantization_test.rs: RTT accuracy within 0.1% or 0.1ms
// - chunked_v1_timing_test.rs: Cumulative drift ≤ 20ms testing
// - chunked_v1_loss_test.rs: Packet loss preservation accuracy
// - chunked_v1_edge_cases_test.rs: All edge cases and boundary conditions
// - chunked_v1_corruption_test.rs: File corruption resistance (never panic)
// - chunked_v1_adversarial_test.rs: Pathological inputs designed to break format
// - chunked_v1_format_test.rs: Format compliance and cross-platform compatibility
// - chunked_v1_performance_test.rs: Speed and memory benchmarks
//
// CURRENT STATUS: ✅ Basic round-trip works with minute boundary + nanosecond offset format

use anyhow::Result;
use std::fs::File;
use std::io::Read;
use zzping_press::{RawDataRecord, chunked_v1};

#[derive(Debug)]
struct ErrorStats {
    drift_violations: Vec<DriftViolation>,
    rtt_violations: Vec<RttViolation>,
    max_cumulative_drift_ns: i64,
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
            max_cumulative_drift_ns: 0,
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
        println!("Drift violations: {}", self.total_drift_violations);
        println!("RTT violations: {}", self.total_rtt_violations);
        println!(
            "Max cumulative drift: {}",
            Self::format_duration_ns(self.max_cumulative_drift_ns)
        );
        println!(
            "Max individual drift: {}",
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
    // TODO: This test should be expanded according to the test plan to cover:
    // 1. Multiple test datasets with different characteristics (constant rate, variable rate, packet loss)
    // 2. Edge cases: single ping chunks, empty chunks, clock anomalies
    // 3. Stress testing: large datasets, memory pressure
    // 4. Boundary testing: quantization limits, minute boundaries
    // 5. Error injection: test graceful degradation

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

    let mut cumulative_drift_ns: i64 = 0;
    let mut error_stats = ErrorStats::new();
    const MAX_CUMULATIVE_DRIFT_NS: i64 = 20 * 1_000_000; // 20ms

    // Collect all errors before failing
    for (i, (original, decompressed)) in original_records
        .iter()
        .zip(decompressed_records.iter())
        .enumerate()
    {
        // Check sent_time drift
        let individual_drift = original.sent_nanos as i64 - decompressed.sent_nanos as i64;
        cumulative_drift_ns += individual_drift;

        // Track maximum drifts
        error_stats.max_cumulative_drift_ns = error_stats
            .max_cumulative_drift_ns
            .max(cumulative_drift_ns.abs());
        error_stats.max_individual_drift_ns = error_stats
            .max_individual_drift_ns
            .max(individual_drift.abs());

        // Check for cumulative drift violations
        if cumulative_drift_ns.abs() > MAX_CUMULATIVE_DRIFT_NS {
            error_stats.total_drift_violations += 1;

            // Sample the first few violations for detailed analysis
            if error_stats.drift_violations.len() < 5 {
                error_stats.drift_violations.push(DriftViolation {
                    index: i,
                    cumulative_drift_ns,
                    individual_drift_ns: individual_drift,
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

    // Final assertions with readable error messages
    if error_stats.total_drift_violations > 0 {
        panic!(
            "Found {} timing drift violations! Max cumulative drift: {} (limit: {}). See detailed statistics above.",
            error_stats.total_drift_violations,
            ErrorStats::format_duration_ns(error_stats.max_cumulative_drift_ns),
            ErrorStats::format_duration_ns(MAX_CUMULATIVE_DRIFT_NS)
        );
    }

    if error_stats.total_rtt_violations > 0 {
        panic!(
            "Found {} RTT accuracy violations! Max RTT error: {}. See detailed statistics above.",
            error_stats.total_rtt_violations,
            ErrorStats::format_duration_ns(error_stats.max_rtt_error_ns)
        );
    }

    println!("✅ All accuracy tests passed!");
    println!(
        "   Max cumulative drift: {} (limit: {})",
        ErrorStats::format_duration_ns(error_stats.max_cumulative_drift_ns),
        ErrorStats::format_duration_ns(MAX_CUMULATIVE_DRIFT_NS)
    );
    println!(
        "   Max RTT error: {}",
        ErrorStats::format_duration_ns(error_stats.max_rtt_error_ns)
    );

    Ok(())
}
