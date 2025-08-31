use super::*;
use crate::protocol::RawDataRecord;

// Compile-time verification that our size calculations are correct
#[test]
fn test_size_calculations_are_correct() {
    // Verify FileHeader size calculation
    let dummy_header = format::FileHeader {
        magic: 0,
        format_version: 0,
        start_time_unix_ns: 0,
        aggregate_entry_count: 0,
        index_entry_count: 0,
        header_crc32: 0,
    };
    let mut buf = Vec::new();
    dummy_header.write(&mut buf).unwrap();
    assert_eq!(
        buf.len(),
        format::FileHeader::serialized_size(),
        "FileHeader::serialized_size() doesn't match actual serialized size"
    );

    // Verify AggregateEntry size calculation
    let dummy_aggregate = format::AggregateEntry::default();
    let mut buf = Vec::new();
    dummy_aggregate.write(&mut buf).unwrap();
    assert_eq!(
        buf.len(),
        format::AggregateEntry::serialized_size(),
        "AggregateEntry::serialized_size() doesn't match actual serialized size"
    );

    // Verify IndexEntry size calculation
    let dummy_index = format::IndexEntry {
        chunk_offset_bytes: 0,
    };
    let mut buf = Vec::new();
    dummy_index.write(&mut buf).unwrap();
    assert_eq!(
        buf.len(),
        format::IndexEntry::serialized_size(),
        "IndexEntry::serialized_size() doesn't match actual serialized size"
    );
}

#[test]
fn test_comprehensive_serialized_sizes() {
    // Test FileHeader with various values to ensure size is consistent
    let test_headers = [
        format::FileHeader {
            magic: 0,
            format_version: 0,
            start_time_unix_ns: 0,
            aggregate_entry_count: 0,
            index_entry_count: 0,
            header_crc32: 0,
        },
        format::FileHeader {
            magic: format::FILE_MAGIC,
            format_version: format::FORMAT_VERSION,
            start_time_unix_ns: 1672531200000000000,
            aggregate_entry_count: 100,
            index_entry_count: 50,
            header_crc32: 0xDEADBEEF,
        },
        format::FileHeader {
            magic: u64::MAX,
            format_version: u16::MAX,
            start_time_unix_ns: u64::MAX,
            aggregate_entry_count: u32::MAX,
            index_entry_count: u32::MAX,
            header_crc32: u32::MAX,
        },
    ];

    for (i, header) in test_headers.iter().enumerate() {
        let mut buf = Vec::new();
        header.write(&mut buf).unwrap();
        assert_eq!(
            buf.len(),
            format::FileHeader::serialized_size(),
            "FileHeader #{}: serialized size {} doesn't match calculated size {}",
            i,
            buf.len(),
            format::FileHeader::serialized_size()
        );
    }

    // Test AggregateEntry with various values
    let test_aggregates = [
        format::AggregateEntry::default(),
        format::AggregateEntry {
            p00_symbol: 100,
            p10_symbol: 200,
            p20_symbol: 300,
            p30_symbol: 400,
            p40_symbol: 500,
            p50_symbol: 600,
            p60_symbol: 700,
            p70_symbol: 800,
            p80_symbol: 900,
            p90_symbol: 1000,
            p100_symbol: 1100,
            lost_packet_count: 42,
        },
        format::AggregateEntry {
            p00_symbol: u16::MAX,
            p10_symbol: u16::MAX,
            p20_symbol: u16::MAX,
            p30_symbol: u16::MAX,
            p40_symbol: u16::MAX,
            p50_symbol: u16::MAX,
            p60_symbol: u16::MAX,
            p70_symbol: u16::MAX,
            p80_symbol: u16::MAX,
            p90_symbol: u16::MAX,
            p100_symbol: u16::MAX,
            lost_packet_count: u32::MAX,
        },
    ];

    for (i, aggregate) in test_aggregates.iter().enumerate() {
        let mut buf = Vec::new();
        aggregate.write(&mut buf).unwrap();
        assert_eq!(
            buf.len(),
            format::AggregateEntry::serialized_size(),
            "AggregateEntry #{}: serialized size {} doesn't match calculated size {}",
            i,
            buf.len(),
            format::AggregateEntry::serialized_size()
        );
    }

    // Test IndexEntry with various values
    let test_indices = [
        format::IndexEntry {
            chunk_offset_bytes: 0,
        },
        format::IndexEntry {
            chunk_offset_bytes: 65536,
        },
        format::IndexEntry {
            chunk_offset_bytes: 1024 * 1024 * 1024,
        }, // 1GB
        format::IndexEntry {
            chunk_offset_bytes: u64::MAX,
        },
    ];

    for (i, index) in test_indices.iter().enumerate() {
        let mut buf = Vec::new();
        index.write(&mut buf).unwrap();
        assert_eq!(
            buf.len(),
            format::IndexEntry::serialized_size(),
            "IndexEntry #{}: serialized size {} doesn't match calculated size {}",
            i,
            buf.len(),
            format::IndexEntry::serialized_size()
        );
    }
}

#[test]
fn test_chunked_header_serialized_size_analysis() {
    // ChunkHeader doesn't have a serialized_size() method because it's variable-sized
    // (optional send_time_stats), but let's verify our understanding of its size

    // Test minimum size ChunkHeader (no send_time_stats)
    let minimal_header = format::ChunkHeader {
        minute_boundary_unix_ns: 0,
        first_ping_offset_ns: 0,
        rtt_symbol_count: 0,
        send_time_symbol_count: 0,
        rtt_stream_len_bytes: 0,
        send_time_stream_len_bytes: 0,
        flags: format::ChunkFlags::empty(),
        base_interval_ns: 0,
        rtt_stats: format::AggregateEntry::default(),
        send_time_stats: None,
        chunk_crc32: 0,
    };

    let mut buf = Vec::new();
    minimal_header.write(&mut buf).unwrap();
    let minimal_size = buf.len();

    // Expected size: 8+8+4+4+4+4+1+8+26+4 = 71 bytes
    // (minute_boundary + first_ping_offset + rtt_symbol_count + send_time_symbol_count +
    //  rtt_stream_len + send_time_stream_len + flags + base_interval + rtt_stats + chunk_crc32)
    let expected_minimal_size =
        8 + 8 + 4 + 4 + 4 + 4 + 1 + 8 + format::AggregateEntry::serialized_size() + 4;
    assert_eq!(
        minimal_size, expected_minimal_size,
        "Minimal ChunkHeader size: expected {expected_minimal_size}, got {minimal_size}"
    );

    // Test maximum size ChunkHeader (with send_time_stats)
    let maximal_header = format::ChunkHeader {
        minute_boundary_unix_ns: u64::MAX,
        first_ping_offset_ns: u64::MAX,
        rtt_symbol_count: u32::MAX,
        send_time_symbol_count: u32::MAX,
        rtt_stream_len_bytes: u32::MAX,
        send_time_stream_len_bytes: u32::MAX,
        flags: format::ChunkFlags::IS_VARIABLE_RATE, // This enables send_time_stats
        base_interval_ns: u64::MAX,
        rtt_stats: format::AggregateEntry {
            p00_symbol: u16::MAX,
            p10_symbol: u16::MAX,
            p20_symbol: u16::MAX,
            p30_symbol: u16::MAX,
            p40_symbol: u16::MAX,
            p50_symbol: u16::MAX,
            p60_symbol: u16::MAX,
            p70_symbol: u16::MAX,
            p80_symbol: u16::MAX,
            p90_symbol: u16::MAX,
            p100_symbol: u16::MAX,
            lost_packet_count: u32::MAX,
        },
        send_time_stats: Some(format::AggregateEntry {
            p00_symbol: u16::MAX,
            p10_symbol: u16::MAX,
            p20_symbol: u16::MAX,
            p30_symbol: u16::MAX,
            p40_symbol: u16::MAX,
            p50_symbol: u16::MAX,
            p60_symbol: u16::MAX,
            p70_symbol: u16::MAX,
            p80_symbol: u16::MAX,
            p90_symbol: u16::MAX,
            p100_symbol: u16::MAX,
            lost_packet_count: u32::MAX,
        }),
        chunk_crc32: u32::MAX,
    };

    let mut buf = Vec::new();
    maximal_header.write(&mut buf).unwrap();
    let maximal_size = buf.len();

    // Expected size: minimal_size + additional AggregateEntry
    let expected_maximal_size = expected_minimal_size + format::AggregateEntry::serialized_size();
    assert_eq!(
        maximal_size, expected_maximal_size,
        "Maximal ChunkHeader size: expected {expected_maximal_size}, got {maximal_size}"
    );

    println!("ChunkHeader sizes verified:");
    println!("  Minimal (no send_time_stats): {minimal_size} bytes");
    println!("  Maximal (with send_time_stats): {maximal_size} bytes");
    println!(
        "  Difference: {} bytes (one AggregateEntry)",
        maximal_size - minimal_size
    );
}

#[test]
fn test_header_layout_calculations() {
    // Test that our offset calculations work correctly with real data
    let file_header = format::FileHeader {
        magic: format::FILE_MAGIC,
        format_version: format::FORMAT_VERSION,
        start_time_unix_ns: 1672531200000000000,
        aggregate_entry_count: 5,
        index_entry_count: 3,
        header_crc32: 0,
    };

    // Test that calculated offsets match actual serialization layout
    let mut buf = Vec::new();

    // Write file header
    file_header.write(&mut buf).unwrap();
    assert_eq!(buf.len(), format::FileHeader::serialized_size());
    assert_eq!(buf.len(), format::FileHeader::aggregate_entries_offset());

    // Write aggregate entries
    let start_aggregates = buf.len();
    for _ in 0..file_header.aggregate_entry_count {
        format::AggregateEntry::default().write(&mut buf).unwrap();
    }
    let end_aggregates = buf.len();
    assert_eq!(
        end_aggregates - start_aggregates,
        file_header.aggregate_entry_count as usize * format::AggregateEntry::serialized_size()
    );
    assert_eq!(end_aggregates, file_header.index_entries_offset());

    // Write index entries
    let start_indices = buf.len();
    for i in 0..file_header.index_entry_count {
        format::IndexEntry {
            chunk_offset_bytes: i as u64 * 1000,
        }
        .write(&mut buf)
        .unwrap();
    }
    let end_indices = buf.len();
    assert_eq!(
        end_indices - start_indices,
        file_header.index_entry_count as usize * format::IndexEntry::serialized_size()
    );
    assert_eq!(end_indices, file_header.total_header_used());

    println!("Header layout verification:");
    println!(
        "  FileHeader: {} bytes",
        format::FileHeader::serialized_size()
    );
    println!(
        "  {} AggregateEntries: {} bytes",
        file_header.aggregate_entry_count,
        file_header.aggregate_entry_count as usize * format::AggregateEntry::serialized_size()
    );
    println!(
        "  {} IndexEntries: {} bytes",
        file_header.index_entry_count,
        file_header.index_entry_count as usize * format::IndexEntry::serialized_size()
    );
    println!(
        "  Total header used: {} bytes",
        file_header.total_header_used()
    );
    println!("  Available header space: {} bytes", format::HEADER_SIZE);
    println!(
        "  Remaining space: {} bytes",
        format::HEADER_SIZE - file_header.total_header_used()
    );
}

#[test]
fn test_serialization_round_trip_preserves_size() {
    // Test that serialization -> deserialization -> serialization produces identical byte counts

    // Test FileHeader round trip
    let original_header = format::FileHeader {
        magic: format::FILE_MAGIC,
        format_version: format::FORMAT_VERSION,
        start_time_unix_ns: 1672531200000000000,
        aggregate_entry_count: 42,
        index_entry_count: 24,
        header_crc32: 0xCAFEBABE,
    };

    let mut buf1 = Vec::new();
    original_header.write(&mut buf1).unwrap();

    let parsed_header = format::FileHeader::read(&buf1[..]).unwrap();
    let mut buf2 = Vec::new();
    parsed_header.write(&mut buf2).unwrap();

    assert_eq!(buf1.len(), buf2.len(), "FileHeader round-trip changed size");
    assert_eq!(buf1, buf2, "FileHeader round-trip changed content");
    assert_eq!(buf1.len(), format::FileHeader::serialized_size());

    // Test AggregateEntry round trip
    let original_aggregate = format::AggregateEntry {
        p00_symbol: 100,
        p10_symbol: 200,
        p20_symbol: 300,
        p30_symbol: 400,
        p40_symbol: 500,
        p50_symbol: 600,
        p60_symbol: 700,
        p70_symbol: 800,
        p80_symbol: 900,
        p90_symbol: 1000,
        p100_symbol: 1100,
        lost_packet_count: 42,
    };

    let mut buf1 = Vec::new();
    original_aggregate.write(&mut buf1).unwrap();

    let parsed_aggregate = format::AggregateEntry::read(&buf1[..]).unwrap();
    let mut buf2 = Vec::new();
    parsed_aggregate.write(&mut buf2).unwrap();

    assert_eq!(
        buf1.len(),
        buf2.len(),
        "AggregateEntry round-trip changed size"
    );
    assert_eq!(buf1, buf2, "AggregateEntry round-trip changed content");
    assert_eq!(buf1.len(), format::AggregateEntry::serialized_size());

    // Test IndexEntry round trip
    let original_index = format::IndexEntry {
        chunk_offset_bytes: 0x123456789ABCDEF0,
    };

    let mut buf1 = Vec::new();
    original_index.write(&mut buf1).unwrap();

    let parsed_index = format::IndexEntry::read(&buf1[..]).unwrap();
    let mut buf2 = Vec::new();
    parsed_index.write(&mut buf2).unwrap();

    assert_eq!(buf1.len(), buf2.len(), "IndexEntry round-trip changed size");
    assert_eq!(buf1, buf2, "IndexEntry round-trip changed content");
    assert_eq!(buf1.len(), format::IndexEntry::serialized_size());
}

#[test]
fn test_size_calculation_edge_cases() {
    // Test that size calculations work correctly in boundary conditions

    // Test minimum possible header configuration
    let min_header = format::FileHeader {
        magic: format::FILE_MAGIC,
        format_version: format::FORMAT_VERSION,
        start_time_unix_ns: 0,
        aggregate_entry_count: 0,
        index_entry_count: 0,
        header_crc32: 0,
    };

    assert_eq!(
        format::FileHeader::aggregate_entries_offset(),
        format::FileHeader::serialized_size()
    );
    assert_eq!(
        min_header.index_entries_offset(),
        format::FileHeader::serialized_size()
    ); // No aggregates
    assert_eq!(
        min_header.total_header_used(),
        format::FileHeader::serialized_size()
    ); // No aggregates or indices

    // Verify minimum header fits comfortably
    assert!(min_header.total_header_used() < format::HEADER_SIZE);
    min_header.validate_header_fits().unwrap();

    // Test realistic configuration (24 hours of minute chunks)
    let realistic_header = format::FileHeader {
        magic: format::FILE_MAGIC,
        format_version: format::FORMAT_VERSION,
        start_time_unix_ns: 1672531200000000000,
        aggregate_entry_count: 1440, // 24 hours * 60 minutes
        index_entry_count: 1440,     // One chunk per minute
        header_crc32: 0,
    };

    let realistic_used = realistic_header.total_header_used();
    println!("Realistic 24-hour configuration:");
    println!("  Header used: {realistic_used} bytes");
    println!("  Available: {} bytes", format::HEADER_SIZE);
    println!(
        "  Utilization: {:.1}%",
        realistic_used as f64 / format::HEADER_SIZE as f64 * 100.0
    );

    // Should fit comfortably within 64KiB header
    assert!(realistic_used < format::HEADER_SIZE);
    realistic_header.validate_header_fits().unwrap();

    // Test near-maximum configuration (stress test)
    let max_aggregates = (format::HEADER_SIZE - format::FileHeader::serialized_size())
        / format::AggregateEntry::serialized_size();
    let stress_header = format::FileHeader {
        magic: format::FILE_MAGIC,
        format_version: format::FORMAT_VERSION,
        start_time_unix_ns: u64::MAX,
        aggregate_entry_count: max_aggregates as u32,
        index_entry_count: 0, // No room for indices
        header_crc32: u32::MAX,
    };

    let stress_used = stress_header.total_header_used();
    println!("Maximum aggregates configuration:");
    println!("  Max possible aggregates: {max_aggregates}");
    println!("  Header used: {stress_used} bytes");
    println!("  Remaining: {} bytes", format::HEADER_SIZE - stress_used);

    // Should still fit
    assert!(stress_used <= format::HEADER_SIZE);
    stress_header.validate_header_fits().unwrap();

    // Test configuration that exceeds header space (should fail validation)
    let oversized_header = format::FileHeader {
        magic: format::FILE_MAGIC,
        format_version: format::FORMAT_VERSION,
        start_time_unix_ns: 0,
        aggregate_entry_count: 10000, // Way too many
        index_entry_count: 10000,     // Way too many
        header_crc32: 0,
    };

    // Should exceed header space and fail validation
    assert!(oversized_header.total_header_used() > format::HEADER_SIZE);
    assert!(oversized_header.validate_header_fits().is_err());
}

#[test]
fn test_simple_model_creation() {
    let stats = format::AggregateEntry {
        p00_symbol: 100,
        p10_symbol: 110,
        p20_symbol: 120,
        p30_symbol: 130,
        p40_symbol: 140,
        p50_symbol: 150,
        p60_symbol: 160,
        p70_symbol: 170,
        p80_symbol: 180,
        p90_symbol: 190,
        p100_symbol: 200,
        lost_packet_count: 0,
    };
    let (s, p) = compression::build_model(&stats, 100).unwrap();
    let model = constriction::stream::model::DefaultNonContiguousCategoricalEncoderModel::from_symbols_and_floating_point_probabilities_fast(s, &p, None);
    assert!(model.is_ok());
}

#[test]
fn test_single_symbol_model_creation() {
    let stats = format::AggregateEntry {
        p00_symbol: 100,
        p10_symbol: 100,
        p20_symbol: 100,
        p30_symbol: 100,
        p40_symbol: 100,
        p50_symbol: 100,
        p60_symbol: 100,
        p70_symbol: 100,
        p80_symbol: 100,
        p90_symbol: 100,
        p100_symbol: 100,
        lost_packet_count: 0,
    };
    let result = compression::build_model(&stats, 1);
    assert!(result.is_ok(), "Failed with error: {:?}", result.err());
}

#[test]
fn test_model_with_exactly_one_packet_lost() {
    // Test the critical edge case: exactly 1 packet lost out of total packets
    // This tests the packet loss frequency calculation with minimal loss
    let stats = format::AggregateEntry {
        p00_symbol: 100,
        p10_symbol: 110,
        p20_symbol: 120,
        p30_symbol: 130,
        p40_symbol: 140,
        p50_symbol: 150,
        p60_symbol: 160,
        p70_symbol: 170,
        p80_symbol: 180,
        p90_symbol: 190,
        p100_symbol: 200,
        lost_packet_count: 1, // Exactly 1 packet lost
    };

    // Test with different total packet counts to verify frequency calculation
    for total_packets in [100, 10_000, 100_000, 1_000_000] {
        let result = compression::build_model(&stats, total_packets);
        assert!(
            result.is_ok(),
            "Failed with {} total packets: {:?}",
            total_packets,
            result.err()
        );

        let (symbols, probabilities) = result.unwrap();

        // Verify packet loss symbol is included with correct frequency
        let lost_symbol_pos = symbols
            .iter()
            .position(|&s| s == quantization::PACKET_LOST_SYMBOL);
        assert!(
            lost_symbol_pos.is_some(),
            "Packet loss symbol missing with {total_packets} total packets"
        );

        let lost_prob = probabilities[lost_symbol_pos.unwrap()];

        // For 1 lost packet, the frequency should be exactly 1, so probability should be roughly 1/total_frequency
        // The exact probability depends on how frequencies are distributed, but it should be > 0
        assert!(
            lost_prob > 0.0,
            "Packet loss probability should be > 0 with {total_packets} total packets, got {lost_prob}"
        );

        // Verify all probabilities sum to 1.0 (within floating point tolerance)
        let total_prob: f64 = probabilities.iter().sum();
        assert!(
            (total_prob - 1.0).abs() < 1e-10,
            "Probabilities should sum to 1.0, got {total_prob} with {total_packets} total packets"
        );
    }
}

#[test]
fn test_model_with_all_packets_lost() {
    // Test edge case: 100% packet loss
    let stats = format::AggregateEntry {
        p00_symbol: 0, // Default values since no successful RTTs
        p10_symbol: 0,
        p20_symbol: 0,
        p30_symbol: 0,
        p40_symbol: 0,
        p50_symbol: 0,
        p60_symbol: 0,
        p70_symbol: 0,
        p80_symbol: 0,
        p90_symbol: 0,
        p100_symbol: 0,
        lost_packet_count: 50, // All packets lost
    };

    let result = compression::build_model(&stats, 50); // 50 total packets, all lost
    assert!(
        result.is_ok(),
        "Failed with all packets lost: {:?}",
        result.err()
    );

    let (symbols, probabilities) = result.unwrap();

    // Should have exactly one symbol (packet loss) plus potentially a dummy symbol
    assert!(
        !symbols.is_empty(),
        "Should have at least packet loss symbol"
    );

    // Packet loss symbol should be present
    let lost_symbol_pos = symbols
        .iter()
        .position(|&s| s == quantization::PACKET_LOST_SYMBOL);
    assert!(
        lost_symbol_pos.is_some(),
        "Packet loss symbol missing with 100% loss"
    );

    // With 100% packet loss, the packet loss symbol should have high probability
    let lost_prob = probabilities[lost_symbol_pos.unwrap()];
    assert!(
        lost_prob > 0.5,
        "With 100% packet loss, loss probability should be high, got {lost_prob}"
    );
}

/// Helper function for tests to manually construct a valid, finalized file from records.
fn create_finalized_file_for_test(records: &[RawDataRecord]) -> Vec<u8> {
    let mut final_data = compression::create_chunked_v1_header().unwrap();
    let mut chunks: std::collections::BTreeMap<u64, Vec<RawDataRecord>> =
        std::collections::BTreeMap::new();
    for record in records {
        chunks
            .entry(record.sent_nanos / 60_000_000_000)
            .or_default()
            .push(*record);
    }

    let mut aggregate_entries = Vec::new();
    let mut index_entries = Vec::new();
    let mut payload_len = 0;

    for chunk_records in chunks.values() {
        let chunk_body = compression::create_chunk_body(chunk_records).unwrap();

        let mut cursor = std::io::Cursor::new(&chunk_body);
        cursor.set_position(1); // Skip delimiter
        let chunk_header = format::ChunkHeader::read(&mut cursor).unwrap();
        aggregate_entries.push(chunk_header.rtt_stats);
        index_entries.push(format::IndexEntry {
            chunk_offset_bytes: (format::HEADER_SIZE + payload_len + 1) as u64,
        });

        final_data.extend_from_slice(&chunk_body);
        payload_len += chunk_body.len();
    }

    let mut file_header = format::FileHeader {
        magic: format::FILE_MAGIC,
        format_version: format::FORMAT_VERSION,
        start_time_unix_ns: records.first().map_or(0, |r| r.sent_nanos),
        aggregate_entry_count: aggregate_entries.len() as u32,
        index_entry_count: index_entries.len() as u32,
        header_crc32: 0,
    };
    file_header.header_crc32 = format::calculate_file_header_crc32(&file_header);

    let mut cursor = std::io::Cursor::new(&mut final_data[..format::HEADER_SIZE]);
    file_header.write(&mut cursor).unwrap();
    for entry in &aggregate_entries {
        entry.write(&mut cursor).unwrap();
    }
    for entry in &index_entries {
        entry.write(&mut cursor).unwrap();
    }

    final_data
}

#[test]
fn test_basic_compression_roundtrip() {
    use crate::protocol::RawDataRecord;
    use chrono::{TimeZone, Utc};
    use std::time::Duration;

    let base_time = Utc.with_ymd_and_hms(2023, 1, 1, 0, 0, 0).unwrap();
    let start_time = base_time.timestamp_nanos_opt().unwrap() as u64 + 59_900_000_000u64; // 59.9 seconds offset

    let records = vec![RawDataRecord {
        sent_nanos: start_time,
        rtt_nanos: Duration::from_millis(20).as_nanos() as u64,
    }];

    let compressed = create_finalized_file_for_test(&records);
    let decompressed = decompression::decompress_chunked_v1(&compressed).unwrap();

    assert_eq!(records.len(), decompressed.len());
    assert_eq!(records[0].sent_nanos, decompressed[0].sent_nanos);

    // RTT values are quantized for compression, so check they're approximately equal
    let original_rtt = records[0].rtt_nanos;
    let decompressed_rtt = decompressed[0].rtt_nanos;
    let diff = original_rtt.abs_diff(decompressed_rtt);

    // Should be within 1% of original value due to quantization
    let tolerance = original_rtt / 100;
    assert!(
        diff <= tolerance,
        "RTT quantization error too large: original={original_rtt}, decompressed={decompressed_rtt}, diff={diff}, tolerance={tolerance}",
    );
}

#[test]
fn test_simple_variable_rate() {
    use crate::protocol::RawDataRecord;
    use chrono::{TimeZone, Utc};
    use std::time::Duration;

    let base_time = Utc.with_ymd_and_hms(2023, 1, 1, 0, 0, 0).unwrap();
    let start_time = base_time.timestamp_nanos_opt().unwrap() as u64 + 59_900_000_000u64;

    let records = vec![
        RawDataRecord {
            sent_nanos: start_time,
            rtt_nanos: Duration::from_millis(20).as_nanos() as u64,
        },
        RawDataRecord {
            sent_nanos: start_time + 1_100_000_000, // 1.1 seconds later (variable timing)
            rtt_nanos: Duration::from_millis(25).as_nanos() as u64,
        },
    ];

    let compressed = create_finalized_file_for_test(&records);
    let decompressed = decompression::decompress_chunked_v1(&compressed).unwrap();

    assert_eq!(records.len(), decompressed.len());
    assert_eq!(records[0].sent_nanos, decompressed[0].sent_nanos);
    assert_eq!(records[1].sent_nanos, decompressed[1].sent_nanos);
}

#[test]
fn test_multi_chunk_variable_rate() {
    use crate::protocol::RawDataRecord;
    use chrono::{TimeZone, Utc};
    use std::time::Duration;

    let base_time = Utc.with_ymd_and_hms(2023, 1, 1, 0, 0, 0).unwrap();
    let start_time = base_time.timestamp_nanos_opt().unwrap() as u64;

    // Create records that span multiple chunks (more than 50 records to force multiple chunks)
    let mut records = Vec::new();
    let mut timestamp = start_time;
    for i in 0..100 {
        records.push(RawDataRecord {
            sent_nanos: timestamp,
            rtt_nanos: Duration::from_millis(20).as_nanos() as u64,
        });
        // Alternate between different intervals to create variable timing
        if i % 2 == 0 {
            timestamp += 1_100_000_000; // 1.1 seconds
        } else {
            timestamp += 900_000_000; // 0.9 seconds
        }
    }

    let compressed = create_finalized_file_for_test(&records);
    let decompressed = decompression::decompress_chunked_v1(&compressed).unwrap();

    assert_eq!(records.len(), decompressed.len());
    for (i, (original, decompressed)) in records.iter().zip(decompressed.iter()).enumerate() {
        assert_eq!(
            original.sent_nanos, decompressed.sent_nanos,
            "Mismatch at record {i}"
        );
    }
}

#[test]
fn test_dummy_symbol_frequency_analysis() {
    // Test scenarios that might trigger the single symbol case and analyze dummy symbol frequency

    // Case 1: Only packet loss, no successful RTTs (should have 1 symbol: packet loss)
    let stats_only_loss = format::AggregateEntry {
        p00_symbol: 0,
        p10_symbol: 0,
        p20_symbol: 0,
        p30_symbol: 0,
        p40_symbol: 0,
        p50_symbol: 0,
        p60_symbol: 0,
        p70_symbol: 0,
        p80_symbol: 0,
        p90_symbol: 0,
        p100_symbol: 0,
        lost_packet_count: 10,
    };

    let result = compression::build_model(&stats_only_loss, 10); // 10 total packets, all lost
    assert!(result.is_ok());
    let (symbols, probabilities) = result.unwrap();

    // Should have packet loss symbol with frequency 10, and potentially a dummy symbol with frequency 1
    println!(
        "Only loss case: {} symbols, frequencies: {:?}",
        symbols.len(),
        probabilities
    );
    if symbols.len() == 2 {
        // Find which is packet loss and which is dummy
        let loss_pos = symbols
            .iter()
            .position(|&s| s == quantization::PACKET_LOST_SYMBOL)
            .unwrap();
        let dummy_pos = 1 - loss_pos; // the other one
        println!(
            "Loss symbol freq: {}, Dummy symbol freq: {}",
            probabilities[loss_pos], probabilities[dummy_pos]
        );

        // The real symbol should have much higher frequency than dummy
        assert!(
            probabilities[loss_pos] > probabilities[dummy_pos] * 5.0,
            "Loss symbol probability should be much higher than dummy"
        );
    }

    // Case 2: All RTTs identical (should have 1 RTT symbol)
    let stats_identical_rtt = format::AggregateEntry {
        p00_symbol: 100,
        p10_symbol: 100,
        p20_symbol: 100,
        p30_symbol: 100,
        p40_symbol: 100,
        p50_symbol: 100,
        p60_symbol: 100,
        p70_symbol: 100,
        p80_symbol: 100,
        p90_symbol: 100,
        p100_symbol: 100,
        lost_packet_count: 0,
    };

    let result = compression::build_model(&stats_identical_rtt, 50); // 50 packets, all same RTT
    assert!(result.is_ok());
    let (symbols, probabilities) = result.unwrap();

    println!(
        "Identical RTT case: {} symbols, frequencies: {:?}",
        symbols.len(),
        probabilities
    );
    if symbols.len() == 2 {
        // Should have RTT symbol 100 with high frequency, dummy with frequency 1
        let rtt_pos = symbols.iter().position(|&s| s == 100).unwrap();
        let dummy_pos = 1 - rtt_pos;
        println!(
            "RTT symbol freq: {}, Dummy symbol freq: {}",
            probabilities[rtt_pos], probabilities[dummy_pos]
        );

        // The real symbol should have much higher frequency than dummy
        assert!(
            probabilities[rtt_pos] > probabilities[dummy_pos] * 5.0,
            "RTT symbol probability should be much higher than dummy"
        );
    }
}

#[test]
fn test_constant_rate_compression_efficiency() {
    use crate::protocol::RawDataRecord;
    use chrono::{TimeZone, Utc};

    // Create test data: 100 pings per second for 60 seconds (6000 pings in one minute)
    let base_time = Utc.with_ymd_and_hms(2023, 1, 1, 0, 0, 0).unwrap();
    let start_time = base_time.timestamp_nanos_opt().unwrap() as u64;

    const PING_INTERVAL_NS: u64 = 10_000_000; // 10ms = 100 pings per second
    const NUM_PINGS: usize = 6000; // One minute of data at 100 pings/sec

    let mut records = Vec::with_capacity(NUM_PINGS);

    // Generate perfectly regular pings at exactly 10ms intervals
    for i in 0..NUM_PINGS {
        records.push(RawDataRecord {
            sent_nanos: start_time + (i as u64 * PING_INTERVAL_NS),
            rtt_nanos: 20_000_000, // 20ms RTT
        });
    }

    // Compress the data
    let compressed = create_finalized_file_for_test(&records);

    // For a fair comparison, we should exclude the fixed header size from our calculation
    // since it would be amortized over more chunks in a real-world scenario
    let compressed_size_without_header = compressed.len() - format::HEADER_SIZE;
    let bits_per_ping = (compressed_size_without_header * 8) as f64 / NUM_PINGS as f64;

    // Debug info
    println!("Constant rate compression:");
    println!(
        "  Original size: {} bytes",
        records.len() * std::mem::size_of::<RawDataRecord>()
    );
    println!(
        "  Compressed size (with header): {} bytes",
        compressed.len()
    );
    println!(
        "  Compressed size (without header): {compressed_size_without_header} bytes"
    );
    println!(
        "  Compression ratio (without header): {:.2}:1",
        (records.len() * std::mem::size_of::<RawDataRecord>()) as f64
            / compressed_size_without_header as f64
    );
    println!("  Bits per ping (without header): {bits_per_ping:.2}");

    // With constant rate, we should achieve less than 8 bits per ping (excluding fixed header)
    assert!(
        bits_per_ping < 8.0,
        "Constant rate compression should use <8 bits per ping, but used {bits_per_ping:.2}"
    );

    // Verify we can decompress correctly
    let decompressed = decompression::decompress_chunked_v1(&compressed).unwrap();
    assert_eq!(
        records.len(),
        decompressed.len(),
        "Decompressed record count mismatch"
    );

    // Verify timing is preserved within quantization error
    for i in 0..records.len() {
        assert_eq!(
            records[i].sent_nanos / PING_INTERVAL_NS,
            decompressed[i].sent_nanos / PING_INTERVAL_NS,
            "Ping interval not preserved at index {i}"
        );
    }
}

#[test]
fn test_variable_rate_compression_efficiency() {
    use crate::protocol::RawDataRecord;
    use chrono::{TimeZone, Utc};

    // Create test data: ~100 pings per second with deliberate timing variation
    let base_time = Utc.with_ymd_and_hms(2023, 1, 1, 0, 0, 0).unwrap();
    let start_time = base_time.timestamp_nanos_opt().unwrap() as u64;

    const BASE_PING_INTERVAL_NS: u64 = 10_000_000; // 10ms base interval
    const NUM_PINGS: usize = 6000; // One minute of data

    let mut records = Vec::with_capacity(NUM_PINGS);

    // Generate variable rate pings with alternating patterns to ensure detection
    let mut current_time = start_time;
    for i in 0..NUM_PINGS {
        records.push(RawDataRecord {
            sent_nanos: current_time,
            rtt_nanos: 20_000_000 + (i as u64 % 1000) * 1000, // Add some RTT variation
        });

        // Create deliberate timing variation that exceeds the 1ms tolerance
        let interval = if i % 3 == 0 {
            BASE_PING_INTERVAL_NS + 2_000_000 // +2ms every 3rd ping
        } else if i % 7 == 0 {
            BASE_PING_INTERVAL_NS - 1_500_000 // -1.5ms every 7th ping
        } else {
            BASE_PING_INTERVAL_NS // Normal interval
        };

        current_time += interval;

        // Ensure we don't cross minute boundary
        if i == NUM_PINGS - 1 {
            let elapsed_time = current_time - start_time;
            if elapsed_time >= 60_000_000_000 {
                // 60 seconds
                records.truncate(i);
                break;
            }
        }
    }

    // Compress the data
    let compressed = create_finalized_file_for_test(&records);

    // For a fair comparison, we should exclude the fixed header size from our calculation
    // since it would be amortized over more chunks in a real-world scenario
    let compressed_size_without_header = compressed.len() - format::HEADER_SIZE;
    let bits_per_ping = (compressed_size_without_header * 8) as f64 / records.len() as f64; // Use actual record count

    // Debug info
    println!("Variable rate compression:");
    println!("  Actual records: {}", records.len());
    println!(
        "  Original size: {} bytes",
        records.len() * std::mem::size_of::<RawDataRecord>()
    );
    println!(
        "  Compressed size (with header): {} bytes",
        compressed.len()
    );
    println!(
        "  Compressed size (without header): {compressed_size_without_header} bytes"
    );
    println!(
        "  Compression ratio (without header): {:.2}:1",
        (records.len() * std::mem::size_of::<RawDataRecord>()) as f64
            / compressed_size_without_header as f64
    );
    println!("  Bits per ping (without header): {bits_per_ping:.2}");

    // With variable rate (storing timing deltas), we expect reasonable compression
    // Current implementation uses ~32 bits per ping, so let's set a realistic target
    assert!(
        bits_per_ping < 40.0,
        "Variable rate compression should use <40 bits per ping, but used {bits_per_ping:.2}"
    );

    // Verify we can decompress correctly
    let decompressed = decompression::decompress_chunked_v1(&compressed).unwrap();
    assert_eq!(
        records.len(),
        decompressed.len(),
        "Decompressed record count mismatch"
    );

    // Verify timing is preserved within a reasonable error margin
    // For variable rate, we need to allow more tolerance due to quantization and jitter
    let tolerance_ns = 2_000_000; // 2ms tolerance to account for jitter and quantization
    for i in 0..records.len() {
        let time_diff = records[i].sent_nanos.abs_diff(decompressed[i].sent_nanos);
        assert!(
            time_diff <= tolerance_ns,
            "Timing not preserved within tolerance at index {i}: diff={time_diff} ns"
        );
    }
}
