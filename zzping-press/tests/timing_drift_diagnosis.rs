use anyhow::Result;
use std::fs::File;
use std::io::Read;
use zzping_press::{RawDataRecord, chunked_v1};

#[test]
fn diagnose_timing_drift() -> Result<()> {
    // Load the same fixture file as the failing test
    let mut file = File::open("tests/fixtures/ten-minutes.dat")?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;

    // Skip the 16-byte header to get to the records
    let records_data = &buffer[16..];
    let mut original_records: Vec<RawDataRecord> = records_data
        .chunks_exact(16)
        .map(|chunk| RawDataRecord {
            sent_nanos: u64::from_le_bytes(chunk[0..8].try_into().unwrap()),
            rtt_nanos: u64::from_le_bytes(chunk[8..16].try_into().unwrap()),
        })
        .collect();
    original_records.sort();

    println!("=== TIMING DRIFT DIAGNOSIS ===");
    println!("Total records: {}", original_records.len());

    // Analyze original timing intervals
    let mut original_intervals = Vec::new();
    for i in 1..original_records.len() {
        let interval = original_records[i].sent_nanos - original_records[i - 1].sent_nanos;
        original_intervals.push(interval);
    }

    // Calculate statistics on original intervals
    let mut sorted_intervals = original_intervals.clone();
    sorted_intervals.sort();
    let median_interval = sorted_intervals[sorted_intervals.len() / 2];
    let mode_interval = calculate_mode_interval(&original_intervals);

    println!("Original timing analysis:");
    println!(
        "  Median interval: {} ns ({:.3} ms)",
        median_interval,
        median_interval as f64 / 1_000_000.0
    );
    println!(
        "  Mode interval: {} ns ({:.3} ms)",
        mode_interval,
        mode_interval as f64 / 1_000_000.0
    );

    // Show interval distribution around the mode
    let mut interval_counts = std::collections::HashMap::new();
    for &interval in &original_intervals {
        *interval_counts.entry(interval).or_insert(0) += 1;
    }

    let mut sorted_counts: Vec<_> = interval_counts.iter().collect();
    sorted_counts.sort_by_key(|(_, count)| std::cmp::Reverse(**count));

    println!("  Top 10 most common intervals:");
    for (i, (interval, count)) in sorted_counts.iter().take(10).enumerate() {
        println!(
            "    {}: {} ns ({:.3} ms) appears {} times",
            i + 1,
            interval,
            **interval as f64 / 1_000_000.0,
            count
        );
    }

    // Analyze timing consistency - check how many intervals are exactly the mode
    let exact_mode_count = original_intervals
        .iter()
        .filter(|&&i| i == mode_interval)
        .count();
    let within_1ms_count = original_intervals
        .iter()
        .filter(|&&i| (i as i64 - mode_interval as i64).abs() <= 1_000_000)
        .count();

    println!(
        "  Intervals exactly matching mode: {} out of {} ({:.2}%)",
        exact_mode_count,
        original_intervals.len(),
        exact_mode_count as f64 * 100.0 / original_intervals.len() as f64
    );
    println!(
        "  Intervals within 1ms of mode: {} out of {} ({:.2}%)",
        within_1ms_count,
        original_intervals.len(),
        within_1ms_count as f64 * 100.0 / original_intervals.len() as f64
    );

    // Now compress and decompress
    let compressed_data = chunked_v1::compress_chunked_v1(&original_records)?;
    let mut decompressed_records = chunked_v1::decompress_chunked_v1(&compressed_data)?;
    decompressed_records.sort();

    // Analyze decompressed timing
    let mut decompressed_intervals = Vec::new();
    for i in 1..decompressed_records.len() {
        let interval = decompressed_records[i].sent_nanos - decompressed_records[i - 1].sent_nanos;
        decompressed_intervals.push(interval);
    }

    // Compare interval distributions
    let mut decompressed_interval_counts = std::collections::HashMap::new();
    for &interval in &decompressed_intervals {
        *decompressed_interval_counts.entry(interval).or_insert(0) += 1;
    }

    let mut decompressed_sorted_counts: Vec<_> = decompressed_interval_counts.iter().collect();
    decompressed_sorted_counts.sort_by_key(|(_, count)| std::cmp::Reverse(**count));

    println!("\nDecompressed timing analysis:");
    println!("  Top 10 most common decompressed intervals:");
    for (i, (interval, count)) in decompressed_sorted_counts.iter().take(10).enumerate() {
        println!(
            "    {}: {} ns ({:.3} ms) appears {} times",
            i + 1,
            interval,
            **interval as f64 / 1_000_000.0,
            count
        );
    }

    // Track cumulative drift over time
    let mut cumulative_drift = 0i64;
    let mut max_drift = 0i64;
    let mut drift_samples = Vec::new();

    println!("\nDrift analysis (first 20 records):");
    for i in 0..20.min(original_records.len()) {
        let individual_drift =
            original_records[i].sent_nanos as i64 - decompressed_records[i].sent_nanos as i64;
        cumulative_drift += individual_drift;
        max_drift = max_drift.max(cumulative_drift.abs());

        if i < 20 {
            println!(
                "  Record {}: Original: {} ns, Decompressed: {} ns, Individual drift: {} ns, Cumulative: {} ns",
                i,
                original_records[i].sent_nanos,
                decompressed_records[i].sent_nanos,
                individual_drift,
                cumulative_drift
            );
        }

        // Sample every 1000th record for analysis
        if i % 1000 == 0 {
            drift_samples.push((i, individual_drift, cumulative_drift));
        }
    }

    println!("\nDrift samples every 1000 records:");
    for (index, individual, cumulative) in drift_samples {
        println!(
            "  Record {}: Individual drift = {:.3} ms, Cumulative drift = {:.3} ms",
            index,
            individual as f64 / 1_000_000.0,
            cumulative as f64 / 1_000_000.0
        );
    }

    // Check if all chunks are using constant rate mode
    // We need to examine chunk boundaries to understand the issue
    println!("\nChunk boundary analysis:");
    let first_record_time = original_records[0].sent_nanos;
    let last_record_time = original_records.last().unwrap().sent_nanos;
    let total_duration_ns = last_record_time - first_record_time;
    let expected_chunk_count = (total_duration_ns / (60 * 1_000_000_000)) + 1;

    println!("  First record time: {} ns", first_record_time);
    println!("  Last record time: {} ns", last_record_time);
    println!(
        "  Total duration: {:.3} minutes",
        total_duration_ns as f64 / (60.0 * 1_000_000_000.0)
    );
    println!("  Expected chunk count: {}", expected_chunk_count);

    Ok(())
}

// Helper function - copy from the main code for analysis
fn calculate_mode_interval(intervals: &[u64]) -> u64 {
    if intervals.is_empty() {
        return 1_000_000_000; // Default 1 second
    }

    // For efficiency with large datasets, we'll use a simple approach:
    // Find the median as an approximation of the mode for regular intervals
    let mut sorted_intervals = intervals.to_vec();
    sorted_intervals.sort_unstable();
    sorted_intervals[sorted_intervals.len() / 2]
}
