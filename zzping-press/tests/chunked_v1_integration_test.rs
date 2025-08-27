use anyhow::Result;
use std::fs::File;
use std::io::Read;
use zzping_press::{chunked_v1, RawDataRecord};

#[test]
fn test_round_trip() -> Result<()> {
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

    // 2. Compress the data.
    let compressed_data = chunked_v1::compress_chunked_v1(&original_records)?;

    // 3. Decompress the data.
    let mut decompressed_records = chunked_v1::decompress_chunked_v1(&compressed_data)?;
    decompressed_records.sort();

    // 4. Assert correctness.
    assert_eq!(original_records.len(), decompressed_records.len());

    let mut cumulative_drift_ns: i64 = 0;
    const MAX_CUMULATIVE_DRIFT_NS: i64 = 2 * 1_000_000; // 2ms

    for (i, (original, decompressed)) in original_records.iter().zip(decompressed_records.iter()).enumerate() {
        // Assert sent_time drift
        let drift = original.sent_nanos as i64 - decompressed.sent_nanos as i64;
        cumulative_drift_ns += drift;

        assert!(
            cumulative_drift_ns.abs() <= MAX_CUMULATIVE_DRIFT_NS,
            "Cumulative sent_time drift exceeded 2ms. Drift is {cumulative_drift_ns}ns at index {i}"
        );

        // Assert RTT tolerance
        if original.rtt_nanos == u64::MAX {
            assert_eq!(decompressed.rtt_nanos, u64::MAX, "Packet loss mismatch");
        } else {
            let rtt_diff_ns = (original.rtt_nanos as i64 - decompressed.rtt_nanos as i64).abs();
            let tolerance_ns = (500_000).max((original.rtt_nanos as f64 * 0.002).round() as i64);
            assert!(
                rtt_diff_ns <= tolerance_ns,
                "RTT difference {rtt_diff_ns}ns exceeded tolerance {tolerance_ns}ns"
            );
        }
    }

    Ok(())
}
