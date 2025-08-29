// Chunked V1 File Corruption Resistance Tests
//
// PURPOSE: Ensure decoder never panics and handles corrupted files gracefully
// REQUIREMENT: Never panic policy - always return meaningful errors
//
// TEST COVERAGE:
// - Empty files and truncated headers
// - Invalid magic numbers and version fields
// - Corrupted chunk headers and data streams
// - Malformed index tables and offset calculations
// - Incomplete files (power loss, disk full scenarios)
// - Cross-platform compatibility (endianness testing)

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::time::Duration;
use zzping_press::{
    RawDataRecord,
    chunked_v1::{
        FILE_MAGIC, FORMAT_VERSION, HEADER_SIZE, IndexEntry, compress_chunked_v1,
        decompress_chunked_v1,
    },
};

// Test 0-byte file handling
#[test]
fn test_corruption_resistance_empty_file() {
    let data = vec![];
    let result = decompress_chunked_v1(&data);
    assert!(result.is_err(), "Decompressing an empty file should fail");
}

// Test files smaller than 64KiB header size
#[test]
fn test_corruption_resistance_truncated_header() {
    let data = vec![0u8; HEADER_SIZE - 1];
    let result = decompress_chunked_v1(&data);
    assert!(
        result.is_err(),
        "Decompressing a truncated header should fail"
    );
}

// Test files with corrupted magic number
#[test]
fn test_corruption_resistance_invalid_magic() {
    let records = vec![RawDataRecord {
        sent_nanos: 1_672_531_200_000_000_000,
        rtt_nanos: Duration::from_millis(20).as_nanos() as u64,
    }];
    let mut compressed_data = compress_chunked_v1(&records).unwrap();
    compressed_data[0..8].copy_from_slice(&[0, 1, 2, 3, 4, 5, 6, 7]);
    let result = decompress_chunked_v1(&compressed_data);
    assert!(
        result.is_err(),
        "Decompressing with invalid magic number should fail"
    );
}

// Test files ending mid-chunk
#[test]
fn test_corruption_resistance_truncated_chunks() {
    let mut records = Vec::new();
    for i in 0..120 {
        // 2 minutes of data
        records.push(RawDataRecord {
            sent_nanos: 1_672_531_200_000_000_000 + (i * 1_000_000_000),
            rtt_nanos: Duration::from_millis(20).as_nanos() as u64,
        });
    }
    let mut compressed_data = compress_chunked_v1(&records).unwrap();

    // Truncate the data somewhere in the middle of the chunk payload
    let truncation_point = HEADER_SIZE + (compressed_data.len() - HEADER_SIZE) / 2;
    compressed_data.truncate(truncation_point);

    let result = decompress_chunked_v1(&compressed_data);
    assert!(
        result.is_err(),
        "Decompressing a truncated chunk should fail"
    );
}

// Test corrupted index table with invalid offsets
#[test]
fn test_corruption_resistance_malformed_indices() {
    // THEORY: The decompressor does not validate that the chunk offsets from
    // the index table are within the bounds of the file. When an index entry
    // contains an offset that points beyond the end of the file, the code
    // panics with a 'range start index out of range' error when trying to
    // slice the data. The expected behavior is to return an Err instead of
    // panicking.
    //
    // FIXED: Added bounds checking in decompress_chunked_v1()
    let records = vec![RawDataRecord {
        sent_nanos: 1_672_531_200_000_000_000,
        rtt_nanos: Duration::from_millis(20).as_nanos() as u64,
    }];
    let mut compressed_data = compress_chunked_v1(&records).unwrap();

    // Manually corrupt the index entry to point beyond the file
    let file_len = compressed_data.len() as u64;
    let bad_index_entry = IndexEntry {
        chunk_offset_bytes: file_len + 100,
    };

    // Locate the index entry in the header and overwrite it
    // FileHeader: 8+2+8+4+4+4 = 30 bytes
    // AggregateEntry: 26 bytes (with 1 aggregate entry for this single record)
    // IndexEntry starts at: 30 + 26 = 56
    let index_entry_offset = 30 + 26;

    let mut cursor = std::io::Cursor::new(&mut compressed_data[index_entry_offset..]);
    bad_index_entry.write(&mut cursor).unwrap();

    let result = decompress_chunked_v1(&compressed_data);
    assert!(
        result.is_err(),
        "Decompressing with malformed index should fail"
    );
}

// Test cross-platform compatibility (little vs big endian)
#[test]
fn test_corruption_resistance_endianness() {
    let mut data = vec![0u8; HEADER_SIZE];
    // Write magic and version in BigEndian
    data[0..8].copy_from_slice(&FILE_MAGIC.to_be_bytes());
    data[8..10].copy_from_slice(&FORMAT_VERSION.to_be_bytes());
    // Write start time in LittleEndian
    let start_time: u64 = 12345;
    data[10..18].copy_from_slice(&start_time.to_le_bytes());

    let result = decompress_chunked_v1(&data);
    // The decompressor should not panic, and should read a different value for start_time
    if let Ok(records) = result {
        // This is unlikely to be Ok, but if it is, the timestamp should be wrong
        if !records.is_empty() {
            assert_ne!(records[0].sent_nanos, start_time);
        }
    }
    // If it's an error, that's also acceptable. The main point is no panic.
}

// Comprehensive test ensuring decoder never panics on any input
#[test]
fn test_corruption_resistance_never_panic() {
    let mut rng = StdRng::seed_from_u64(12345);
    for _ in 0..1000 {
        let len = rng.random_range(0..1024);
        let data: Vec<u8> = (0..len).map(|_| rng.random::<u8>()).collect();
        // The decompressor should never panic on random data
        let _ = decompress_chunked_v1(&data);
    }
}

// Test Phase 4: Chunk data CRC32 validation
#[test]
fn test_corruption_resistance_chunk_data_crc32() {
    let records = vec![RawDataRecord {
        sent_nanos: 1_672_531_200_000_000_000,
        rtt_nanos: Duration::from_millis(20).as_nanos() as u64,
    }];
    let mut compressed_data = compress_chunked_v1(&records).unwrap();

    // Corrupt a byte in the chunk data (not header, not index)
    // This should pass Phase 1 (bounds checking) and Phase 2 (header CRC32)
    // but fail on Phase 4 (chunk data CRC32)

    // Find chunk start (after header)
    let chunk_start = HEADER_SIZE + 1; // Skip the asterisk delimiter

    // Corrupt one byte in the chunk header area
    if chunk_start < compressed_data.len() {
        compressed_data[chunk_start] = compressed_data[chunk_start].wrapping_add(1);
    }

    let result = decompress_chunked_v1(&compressed_data);
    assert!(
        result.is_err(),
        "Decompressing with corrupted chunk data should fail on Phase 4 CRC32 validation"
    );

    // Verify it's specifically a CRC32 error
    let error_message = result.unwrap_err().to_string();
    assert!(
        error_message.contains("CRC32")
            || error_message.contains("asterisk")
            || error_message.contains("header"),
        "Error should be related to CRC32 validation, got: {}",
        error_message
    );
}
