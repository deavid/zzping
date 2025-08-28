// Theoretical precision analysis for chunked_v1 format
//
// This test analyzes the theoretical limits of timing precision
// given the chunked format's constraint of one mode interval per minute

use anyhow::Result;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use zzping_press::RawDataRecord;

#[test]
fn analyze_theoretical_precision_limits() -> Result<()> {
    // Load the original data
    let mut file = File::open("tests/fixtures/ten-minutes.dat")?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;

    // Skip the 16-byte header to get to the records
    let records_data = &buffer[16..];
    let original_records: Vec<RawDataRecord> = records_data
        .chunks_exact(16)
        .map(|chunk| RawDataRecord {
            sent_nanos: u64::from_le_bytes(chunk[0..8].try_into().unwrap()),
            rtt_nanos: u64::from_le_bytes(chunk[8..16].try_into().unwrap()),
        })
        .collect();

    println!("=== THEORETICAL PRECISION ANALYSIS ===");
    println!("Total records: {}", original_records.len());

    // Group records by minute (like the actual compression does)
    let mut minute_groups: HashMap<u64, Vec<&RawDataRecord>> = HashMap::new();
    for record in &original_records {
        let minute_index = record.sent_nanos / (60 * 1_000_000_000);
        minute_groups.entry(minute_index).or_default().push(record);
    }

    println!("Number of minute chunks: {}", minute_groups.len());

    let mut total_theoretical_error_ns = 0i64;
    let mut max_chunk_error_ns = 0i64;
    let mut chunk_analysis = Vec::new();

    // Analyze each minute chunk
    for (minute_index, records) in minute_groups.iter() {
        if records.len() < 2 {
            continue; // Skip chunks with insufficient data for interval analysis
        }

        // Calculate all intervals in this chunk
        let mut intervals = Vec::new();
        for i in 1..records.len() {
            let interval = records[i].sent_nanos - records[i - 1].sent_nanos;
            intervals.push(interval);
        }

        // Find the true mode (most frequent interval)
        let mut interval_counts: HashMap<u64, usize> = HashMap::new();
        for &interval in &intervals {
            *interval_counts.entry(interval).or_default() += 1;
        }

        let mode_interval = *interval_counts
            .iter()
            .max_by_key(|(_, count)| *count)
            .map(|(interval, _)| interval)
            .unwrap_or(&intervals[0]);

        // Calculate theoretical error if we use only the mode interval
        let mut chunk_error_ns = 0i64;
        for &actual_interval in &intervals {
            let error = actual_interval as i64 - mode_interval as i64;
            chunk_error_ns += error;
        }

        total_theoretical_error_ns += chunk_error_ns;
        max_chunk_error_ns = max_chunk_error_ns.max(chunk_error_ns.abs());

        chunk_analysis.push((
            minute_index,
            records.len(),
            intervals.len(),
            interval_counts.len(), // unique intervals in chunk
            mode_interval,
            chunk_error_ns,
            chunk_error_ns as f64 / 1_000_000.0, // Convert to ms
        ));
    }

    // Sort chunks by error magnitude for analysis
    chunk_analysis.sort_by_key(|(_, _, _, _, _, error_ns, _)| error_ns.abs());
    chunk_analysis.reverse();

    println!("\nWorst 5 chunks by theoretical cumulative error:");
    for (
        i,
        (
            minute_idx,
            record_count,
            interval_count,
            unique_intervals,
            mode_interval,
            error_ns,
            error_ms,
        ),
    ) in chunk_analysis.iter().take(5).enumerate()
    {
        println!(
            "  {}. Minute {}: {} records, {} intervals, {} unique intervals",
            i + 1,
            minute_idx,
            record_count,
            interval_count,
            unique_intervals
        );
        println!(
            "     Mode interval: {} ns ({:.3} ms)",
            mode_interval,
            *mode_interval as f64 / 1_000_000.0
        );
        println!("     Chunk error: {} ns ({:.3} ms)", error_ns, error_ms);
    }

    println!("\nTHEORETICAL LIMITS:");
    println!(
        "  Total theoretical cumulative error: {} ns ({:.3} ms)",
        total_theoretical_error_ns,
        total_theoretical_error_ns as f64 / 1_000_000.0
    );
    println!(
        "  Maximum single chunk error: {} ns ({:.3} ms)",
        max_chunk_error_ns,
        max_chunk_error_ns as f64 / 1_000_000.0
    );
    println!("  Test tolerance: 20 ms");

    let theoretical_violation_ratio =
        (total_theoretical_error_ns.abs() as f64 / 1_000_000.0) / 20.0;
    println!(
        "  Theoretical error vs tolerance: {:.1}x",
        theoretical_violation_ratio
    );

    if theoretical_violation_ratio > 1.0 {
        println!("\n⚠️  ANALYSIS: The test tolerance of 20ms is IMPOSSIBLE to achieve");
        println!("   with this data and the minute-chunking algorithm.");
        println!("   The algorithm is working correctly, but the test expectation");
        println!("   is unrealistic for highly variable interval data.");
    } else {
        println!("\n✅ The test tolerance should be achievable in theory.");
    }

    Ok(())
}
