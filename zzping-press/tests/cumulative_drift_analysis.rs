// Cumulative Drift Analysis
//
// This test verifies whether the "cumulative drift" metric makes sense
// and analyzes what it's actually measuring.

use anyhow::Result;
use std::fs::File;
use std::io::Read;
use zzping_press::{RawDataRecord, chunked_v1};

#[test]
fn analyze_cumulative_drift_metric() -> Result<()> {
    // Load test data
    let mut file = File::open("tests/fixtures/ten-minutes.dat")?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;

    let records_data = &buffer[16..];
    let original_records: Vec<RawDataRecord> = records_data
        .chunks_exact(16)
        .map(|chunk| RawDataRecord {
            sent_nanos: u64::from_le_bytes(chunk[0..8].try_into().unwrap()),
            rtt_nanos: u64::from_le_bytes(chunk[8..16].try_into().unwrap()),
        })
        .collect();

    println!("=== CUMULATIVE DRIFT METRIC ANALYSIS ===");
    println!("Total records: {}", original_records.len());

    // Compress and decompress
    let compressed_data = chunked_v1::compress_chunked_v1(&original_records)?;
    let decompressed_records = chunked_v1::decompress_chunked_v1(&compressed_data)?;

    // Replicate the exact cumulative drift calculation from the test
    let mut cumulative_drift_ns: i64 = 0;
    let mut max_individual_drift = 0i64;
    let mut violations = 0;

    println!("\nCumulative drift calculation (first 20 records):");
    for i in 0..20.min(original_records.len()) {
        let individual_drift =
            original_records[i].sent_nanos as i64 - decompressed_records[i].sent_nanos as i64;
        cumulative_drift_ns += individual_drift;
        max_individual_drift = max_individual_drift.max(individual_drift.abs());

        println!(
            "  Record {}: orig={} decomp={} individual_drift={} cumulative={}",
            i,
            original_records[i].sent_nanos,
            decompressed_records[i].sent_nanos,
            individual_drift,
            cumulative_drift_ns
        );
    }

    // Calculate full cumulative drift
    cumulative_drift_ns = 0;
    for (original, decompressed) in original_records.iter().zip(decompressed_records.iter()) {
        let individual_drift = original.sent_nanos as i64 - decompressed.sent_nanos as i64;
        cumulative_drift_ns += individual_drift;

        if cumulative_drift_ns.abs() > 20_000_000 {
            violations += 1;
        }
    }

    println!("\nFULL ANALYSIS:");
    println!(
        "  Final cumulative drift: {} ns ({:.3} ms)",
        cumulative_drift_ns,
        cumulative_drift_ns as f64 / 1_000_000.0
    );
    println!(
        "  Max individual drift: {} ns ({:.3} ms)",
        max_individual_drift,
        max_individual_drift as f64 / 1_000_000.0
    );
    println!("  Records exceeding 20ms cumulative: {}", violations);

    // Now let's analyze what this SHOULD be measuring
    println!("\n=== ALTERNATIVE TIMING ANALYSIS ===");

    // Method 1: Compare final timestamps (end-to-end drift)
    let original_duration =
        original_records.last().unwrap().sent_nanos - original_records[0].sent_nanos;
    let decompressed_duration =
        decompressed_records.last().unwrap().sent_nanos - decompressed_records[0].sent_nanos;
    let end_to_end_drift = original_duration as i64 - decompressed_duration as i64;

    println!("Method 1 - End-to-end duration comparison:");
    println!(
        "  Original duration: {} ns ({:.3} ms)",
        original_duration,
        original_duration as f64 / 1_000_000.0
    );
    println!(
        "  Decompressed duration: {} ns ({:.3} ms)",
        decompressed_duration,
        decompressed_duration as f64 / 1_000_000.0
    );
    println!(
        "  End-to-end drift: {} ns ({:.3} ms)",
        end_to_end_drift,
        end_to_end_drift as f64 / 1_000_000.0
    );

    // Method 2: Interval-based drift analysis
    println!("\nMethod 2 - Interval reconstruction analysis:");

    // Calculate original intervals
    let mut original_intervals: Vec<u64> = Vec::new();
    for i in 1..original_records.len() {
        original_intervals
            .push(original_records[i].sent_nanos - original_records[i - 1].sent_nanos);
    }

    // Calculate decompressed intervals
    let mut decompressed_intervals: Vec<u64> = Vec::new();
    for i in 1..decompressed_records.len() {
        decompressed_intervals
            .push(decompressed_records[i].sent_nanos - decompressed_records[i - 1].sent_nanos);
    }

    // Cumulative interval error
    let mut cumulative_interval_error = 0i64;
    let mut max_interval_error = 0i64;
    for (orig_interval, decomp_interval) in
        original_intervals.iter().zip(decompressed_intervals.iter())
    {
        let interval_error = *orig_interval as i64 - *decomp_interval as i64;
        cumulative_interval_error += interval_error;
        max_interval_error = max_interval_error.max(interval_error.abs());
    }

    println!(
        "  Cumulative interval error: {} ns ({:.3} ms)",
        cumulative_interval_error,
        cumulative_interval_error as f64 / 1_000_000.0
    );
    println!(
        "  Max individual interval error: {} ns ({:.3} ms)",
        max_interval_error,
        max_interval_error as f64 / 1_000_000.0
    );

    // Method 3: Reconstruct timing from first timestamp + intervals
    println!("\nMethod 3 - Timing reconstruction verification:");

    let original_start = original_records[0].sent_nanos;
    let decompressed_start = decompressed_records[0].sent_nanos;
    println!(
        "  Start time drift: {} ns ({:.3} ms)",
        original_start as i64 - decompressed_start as i64,
        (original_start as i64 - decompressed_start as i64) as f64 / 1_000_000.0
    );

    // Reconstruct decompressed timing from start + intervals
    let mut reconstructed_time = decompressed_start;
    let mut max_reconstruction_error = 0i64;

    for i in 1..10.min(original_records.len()) {
        reconstructed_time += decompressed_intervals[i - 1];
        let actual_decompressed = decompressed_records[i].sent_nanos;
        let reconstruction_error = reconstructed_time as i64 - actual_decompressed as i64;
        max_reconstruction_error = max_reconstruction_error.max(reconstruction_error.abs());

        if i <= 5 {
            println!(
                "    Record {}: reconstructed={} actual_decomp={} error={}",
                i, reconstructed_time, actual_decompressed, reconstruction_error
            );
        }
    }

    println!(
        "  Max reconstruction error: {} ns",
        max_reconstruction_error
    );

    // VERDICT
    println!("\n=== METRIC VALIDITY ANALYSIS ===");
    println!(
        "Current 'cumulative drift' metric: {:.3} ms",
        cumulative_drift_ns as f64 / 1_000_000.0
    );
    println!(
        "End-to-end duration drift: {:.3} ms",
        end_to_end_drift as f64 / 1_000_000.0
    );
    println!(
        "Cumulative interval error: {:.3} ms",
        cumulative_interval_error as f64 / 1_000_000.0
    );

    let ratio1 = cumulative_drift_ns as f64 / end_to_end_drift as f64;
    let ratio2 = cumulative_drift_ns as f64 / cumulative_interval_error as f64;

    println!("Cumulative drift / End-to-end drift = {:.2}x", ratio1);
    println!("Cumulative drift / Interval error = {:.2}x", ratio2);

    if ratio1 > 10.0 || ratio2 > 10.0 {
        println!("\n❌ VERDICT: The 'cumulative drift' metric appears to be WRONG!");
        println!("   It's measuring something different than timing precision.");
        println!("   Individual timestamp errors are being accumulated incorrectly.");
    } else {
        println!("\n✅ VERDICT: The cumulative drift metric seems reasonable.");
    }

    Ok(())
}
