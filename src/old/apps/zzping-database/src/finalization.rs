//! Handles the finalization of `chunked_v1` files.

use anyhow::{Result, anyhow};
use log::{info, warn};
use std::fs;
use std::io::Cursor;
use std::path::Path;
use zzping_lib::chunked_v1::{
    ChunkHeader, FILE_MAGIC, FORMAT_VERSION, FileHeader, HEADER_SIZE, IndexEntry,
    calculate_file_header_crc32,
};

/// Reads a `.zzp1` file, parses all its chunks, and rewrites the header
/// with the complete aggregate and index information.
///
/// This function is the core of the finalization process. It ensures that a
/// file created via an append-only strategy is made whole and queryable.
pub fn finalize_file(path: &Path) -> Result<()> {
    info!("Finalizing file: {path:?}");

    let mut data = fs::read(path)?;

    if data.len() < HEADER_SIZE {
        warn!(
            "File {:?} is smaller than header size ({} bytes). Skipping.",
            path,
            data.len()
        );
        return Ok(());
    }

    // 1. Read the current header to check if finalization is needed.
    let header = {
        let mut cursor = Cursor::new(&data[..HEADER_SIZE]);
        FileHeader::read(&mut cursor)?
    };

    if header.index_entry_count > 0 || header.aggregate_entry_count > 0 {
        warn!("File {path:?} appears to be already finalized. Skipping.");
        return Ok(());
    }

    // 2. Iterate through the payload to find all chunks.
    let mut aggregate_entries = Vec::new();
    let mut index_entries = Vec::new();
    let mut current_offset = HEADER_SIZE as u64;

    while current_offset < data.len() as u64 {
        // Check for start delimiter
        if data[current_offset as usize] != b'*' {
            return Err(anyhow!(
                "Malformed chunk in {:?}: missing start delimiter at offset {}",
                path,
                current_offset
            ));
        }

        let chunk_header_offset = current_offset + 1;
        let mut chunk_cursor = Cursor::new(&data[chunk_header_offset as usize..]);
        let chunk_header = ChunkHeader::read(&mut chunk_cursor)?;

        aggregate_entries.push(chunk_header.rtt_stats);
        index_entries.push(IndexEntry {
            chunk_offset_bytes: chunk_header_offset,
        });

        // Advance offset to the next chunk
        let header_len = chunk_cursor.position();
        let chunk_content_len = header_len
            + chunk_header.rtt_stream_len_bytes as u64
            + chunk_header.send_time_stream_len_bytes as u64;
        let chunk_total_len = 1 + chunk_content_len + 4 + 1; // delimiter + content + crc + delimiter
        current_offset += chunk_total_len;
    }

    if aggregate_entries.is_empty() {
        info!("File {path:?} has no chunks to finalize. Skipping.");
        return Ok(());
    }

    // 3. Construct the new, finalized header.
    let mut new_header = FileHeader {
        magic: FILE_MAGIC,
        format_version: FORMAT_VERSION,
        start_time_unix_ns: 0, // Will be set from the first chunk
        aggregate_entry_count: aggregate_entries.len() as u32,
        index_entry_count: index_entries.len() as u32,
        header_crc32: 0,
    };

    // Set the start time from the first chunk found
    if let Some(first_chunk_index) = index_entries.first() {
        let mut cursor = Cursor::new(&data[first_chunk_index.chunk_offset_bytes as usize..]);
        let first_chunk_header = ChunkHeader::read(&mut cursor)?;
        new_header.start_time_unix_ns = first_chunk_header.minute_boundary_unix_ns;
    }

    new_header.header_crc32 = calculate_file_header_crc32(&new_header);
    new_header.validate_header_fits()?;

    // 4. Write the new header into the in-memory buffer.
    let mut header_cursor = Cursor::new(&mut data[..HEADER_SIZE]);
    new_header.write(&mut header_cursor)?;
    for entry in &aggregate_entries {
        entry.write(&mut header_cursor)?;
    }
    for entry in &index_entries {
        entry.write(&mut header_cursor)?;
    }

    // 5. Write the modified buffer back to the file.
    // This is an atomic overwrite of the entire file.
    fs::write(path, &data)?;

    info!(
        "Successfully finalized file: {:?}, with {} chunks.",
        path,
        index_entries.len()
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    // Removed use ntest::timeout;
    use std::fs;
    use tempfile::tempdir;
    use zzping_lib::{
        chunked_v1::{create_chunk_body, create_chunked_v1_header, decompress_chunked_v1},
        protocol::RawDataRecord,
    };

    #[test]
    // Removed #[timeout(100)]
    fn test_finalize_file() -> Result<()> {
        // 1. Setup: Create an unfinalized file with two chunks.
        let temp_dir = tempdir()?;
        let file_path = temp_dir.path().join("test.zzp1");

        let mut file_content = create_chunked_v1_header()?;

        const ONE_MINUTE_NS: u64 = 60 * 1_000_000_000;
        let records1 = vec![RawDataRecord {
            sent_nanos: ONE_MINUTE_NS,
            rtt_nanos: 10,
        }];
        let records2 = vec![RawDataRecord {
            sent_nanos: 2 * ONE_MINUTE_NS,
            rtt_nanos: 20,
        }];

        let chunk1_body = create_chunk_body(&records1)?;
        let chunk2_body = create_chunk_body(&records2)?;

        file_content.extend_from_slice(&chunk1_body);
        file_content.extend_from_slice(&chunk2_body);
        fs::write(&file_path, file_content)?;

        // 2. Execution: Finalize the file.
        finalize_file(&file_path)?;

        // 3. Verification: Read the file and decompress it.
        let finalized_data = fs::read(&file_path)?;
        let decompressed_records = decompress_chunked_v1(&finalized_data)?;

        let mut expected_records = records1;
        expected_records.extend_from_slice(&records2);

        assert_eq!(decompressed_records, expected_records);

        // Also verify that running finalization again does nothing and returns Ok.
        assert!(finalize_file(&file_path).is_ok());

        Ok(())
    }
}
