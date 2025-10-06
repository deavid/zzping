//! Contains the logic for decompressing the `chunked_v1` format into `RawDataRecord`s.

use super::compression::build_model;
use super::format::{
    ChunkFlags, ChunkHeader, FILE_MAGIC, FORMAT_VERSION, FileHeader, HEADER_SIZE, IndexEntry,
    calculate_chunk_header_crc32, calculate_file_header_crc32,
};
use super::quantization::{PACKET_LOST_SYMBOL, Quantizer};
use crate::protocol::RawDataRecord;
use anyhow::{Result, anyhow};
use byteorder::{BigEndian, ReadBytesExt};
use constriction::stream::{
    Decode, model::DefaultNonContiguousCategoricalDecoderModel, stack::DefaultAnsCoder,
};
use std::io::{Cursor, Read};

/// Builds a probability model for the ANS decompressor from aggregate statistics.
/// This function mirrors the logic in `compression.rs` to reconstruct the exact
/// same probability model that was used for compression.
fn build_model_for_decode(
    stats: &super::format::AggregateEntry,
    symbol_count: usize,
) -> Result<DefaultNonContiguousCategoricalDecoderModel<u16>> {
    let (symbols_with_freq, probabilities_f64) = build_model(stats, symbol_count)?;
    DefaultNonContiguousCategoricalDecoderModel::from_symbols_and_floating_point_probabilities_fast(
        symbols_with_freq,
        &probabilities_f64,
        None,
    )
    .map_err(|_| anyhow!("Failed to create categorical model"))
}

/// Scans the data for chunk delimiters when the index table is empty.
/// Returns a vector of chunk offsets (positions where chunks start).
fn scan_for_chunks(data: &[u8], start_offset: usize) -> Result<Vec<usize>> {
    let mut offsets = Vec::new();
    let mut pos = start_offset;

    while pos + 1 < data.len() {
        // Look for '*' delimiter
        if data[pos] == b'*' {
            let chunk_start = pos + 1;
            // Try to read the chunk header
            let mut cursor = Cursor::new(&data[chunk_start..]);
            if let Ok(chunk_header) = ChunkHeader::read(&mut cursor) {
                // Validate the chunk header CRC
                let computed_crc = calculate_chunk_header_crc32(&chunk_header);
                if computed_crc == chunk_header.chunk_crc32 {
                    offsets.push(chunk_start);
                    // Skip to after this chunk (header + streams + CRC + end delimiter)
                    let header_size = cursor.position() as usize;
                    pos = chunk_start
                        + header_size
                        + chunk_header.rtt_stream_len_bytes as usize
                        + chunk_header.send_time_stream_len_bytes as usize
                        + 4
                        + 1; // CRC32 + end '*'
                    continue;
                }
            }
        }
        pos += 1;
    }

    Ok(offsets)
}

/// Decompresses a slice of bytes in the `chunked_v1` format into a `Vec<RawDataRecord>`.
///
/// # Process
/// 1. Reads and validates the main file header and its CRC32 checksum.
/// 2. Reads the aggregate and index tables from the header.
/// 3. Iterates through each chunk using the index table.
/// 4. For each chunk, it reads the chunk header and validates its CRC32 checksum.
/// 5. It decompresses the RTT and (if present) send-time data streams using an ANS decoder.
/// 6. It reconstructs the `RawDataRecord`s by combining the decompressed symbols with the
///    metadata from the chunk header.
/// 7. Validates chunk-level CRC32 checksums and delimiters for data integrity.
///
/// # Error Handling
/// This function is designed to be robust against corrupted data. It will return an `Err`
/// if any part of the file format is invalid, including magic numbers, version numbers,
/// checksums, or inconsistent lengths.
pub fn decompress_chunked_v1(data: &[u8]) -> Result<Vec<RawDataRecord>> {
    if data.len() < HEADER_SIZE {
        return Err(anyhow!("Data is smaller than header size"));
    }

    let mut cursor = Cursor::new(&data[..HEADER_SIZE]);
    let file_header = FileHeader::read(&mut cursor)?;

    if file_header.magic != FILE_MAGIC {
        return Err(anyhow!("Invalid magic number"));
    }
    if file_header.format_version != FORMAT_VERSION {
        return Err(anyhow!(
            "Unsupported format version: {}",
            file_header.format_version
        ));
    }

    let computed_crc32 = calculate_file_header_crc32(&file_header);
    if computed_crc32 != file_header.header_crc32 {
        return Err(anyhow!(
            "File header CRC32 mismatch: expected 0x{:08x}, computed 0x{:08x}",
            file_header.header_crc32,
            computed_crc32
        ));
    }

    let mut aggregate_table = Vec::with_capacity(file_header.aggregate_entry_count as usize);
    for _ in 0..file_header.aggregate_entry_count {
        aggregate_table.push(super::format::AggregateEntry::read(&mut cursor)?);
    }

    let mut index_table = Vec::with_capacity(file_header.index_entry_count as usize);
    for _ in 0..file_header.index_entry_count {
        index_table.push(IndexEntry::read(&mut cursor)?);
    }

    let quantizer = Quantizer::new();
    let mut all_records = Vec::new();

    // If index table is empty (file not finalized), scan for chunks
    let chunk_offsets = if index_table.is_empty() {
        scan_for_chunks(data, HEADER_SIZE)?
    } else {
        index_table
            .iter()
            .map(|entry| entry.chunk_offset_bytes as usize)
            .collect()
    };

    for (chunk_index, &chunk_offset) in chunk_offsets.iter().enumerate() {
        let chunk_end = if chunk_index + 1 < chunk_offsets.len() {
            chunk_offsets[chunk_index + 1]
        } else {
            data.len()
        };

        if chunk_offset >= data.len() {
            return Err(anyhow!(
                "Chunk offset {} is beyond file size {}",
                chunk_offset,
                data.len()
            ));
        }

        if chunk_offset == 0 || data[chunk_offset - 1] != b'*' {
            return Err(anyhow!(
                "Missing start asterisk delimiter for chunk {} at offset {}",
                chunk_index,
                chunk_offset
            ));
        }

        let mut chunk_cursor = Cursor::new(&data[chunk_offset..]);
        let chunk_header = ChunkHeader::read(&mut chunk_cursor)?;

        let computed_chunk_crc32 = calculate_chunk_header_crc32(&chunk_header);
        if computed_chunk_crc32 != chunk_header.chunk_crc32 {
            return Err(anyhow!(
                "Chunk {} header CRC32 mismatch: expected 0x{:08x}, computed 0x{:08x}",
                chunk_index,
                chunk_header.chunk_crc32,
                computed_chunk_crc32
            ));
        }

        let header_len = chunk_cursor.position() as usize;
        let rtt_stream_start = chunk_offset + header_len;
        let rtt_stream_end = rtt_stream_start + chunk_header.rtt_stream_len_bytes as usize;

        if rtt_stream_end > chunk_end {
            return Err(anyhow!(
                "RTT stream extends beyond chunk boundary: {} > {} (chunk {} boundary)",
                rtt_stream_end,
                chunk_end,
                chunk_index
            ));
        }

        let rtt_data_u8 = &data[rtt_stream_start..rtt_stream_end];
        let rtt_symbols = if chunk_header.rtt_stream_len_bytes > 0 {
            let rtt_data_u32: Vec<u32> = rtt_data_u8
                .chunks_exact(4)
                .map(|b| u32::from_be_bytes(b.try_into().unwrap()))
                .collect();

            let rtt_model = build_model_for_decode(
                &chunk_header.rtt_stats,
                chunk_header.rtt_symbol_count as usize,
            )?;
            let mut decoder = DefaultAnsCoder::from_compressed(rtt_data_u32)
                .map_err(|_| anyhow!("Invalid compressed data for RTT stream"))?;
            let symbols = decoder
                .decode_iid_symbols(chunk_header.rtt_symbol_count as usize, &rtt_model)
                .collect::<Result<Vec<_>, _>>()?;
            if symbols.len() != chunk_header.rtt_symbol_count as usize {
                return Err(anyhow!(
                    "RTT symbol count mismatch in chunk {}: expected {}, decoded {}",
                    chunk_index,
                    chunk_header.rtt_symbol_count,
                    symbols.len()
                ));
            }
            symbols
        } else {
            vec![chunk_header.rtt_stats.p00_symbol; chunk_header.rtt_symbol_count as usize]
        };

        let chunk_start_time =
            chunk_header.minute_boundary_unix_ns + chunk_header.first_ping_offset_ns;
        let mut current_sent_nanos = chunk_start_time;
        let mut send_time_data: Option<Vec<u16>> = None;

        if chunk_header.flags.contains(ChunkFlags::IS_VARIABLE_RATE) {
            let send_time_stream_start = rtt_stream_end;
            let send_time_stream_end =
                send_time_stream_start + chunk_header.send_time_stream_len_bytes as usize;

            if send_time_stream_end > chunk_end {
                return Err(anyhow!(
                    "Send time stream for chunk {} extends beyond chunk boundary: {} > {}",
                    chunk_index,
                    send_time_stream_end,
                    chunk_end
                ));
            }

            let mut time_cursor = Cursor::new(&data[send_time_stream_start..send_time_stream_end]);
            let timing_symbol_count = time_cursor.read_u32::<BigEndian>()? as usize;
            let mut timing_symbols_vec = Vec::with_capacity(timing_symbol_count);
            for _ in 0..timing_symbol_count {
                timing_symbols_vec.push(time_cursor.read_u16::<BigEndian>()?);
            }
            let mut timing_frequencies = Vec::with_capacity(timing_symbol_count);
            for _ in 0..timing_symbol_count {
                timing_frequencies.push(time_cursor.read_u32::<BigEndian>()?);
            }
            let total_freq: u32 = timing_frequencies.iter().sum();
            let timing_probabilities: Vec<f64> = timing_frequencies
                .iter()
                .map(|&f| f as f64 / total_freq as f64)
                .collect();
            let timing_data_len = time_cursor.read_u32::<BigEndian>()? as usize;
            let mut timing_data_bytes = vec![0u8; timing_data_len];
            time_cursor.read_exact(&mut timing_data_bytes)?;
            let timing_data_u32: Vec<u32> = timing_data_bytes
                .chunks_exact(4)
                .map(|chunk| u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                .collect();
            let timing_model = DefaultNonContiguousCategoricalDecoderModel::from_symbols_and_floating_point_probabilities_fast(timing_symbols_vec, &timing_probabilities, None).map_err(|_| anyhow!("Failed to create timing decoder model"))?;
            let mut timing_decoder = DefaultAnsCoder::from_compressed(timing_data_u32)
                .map_err(|_| anyhow!("Failed to create timing decoder"))?;
            let timing_symbols: Vec<u16> = timing_decoder
                .decode_iid_symbols(chunk_header.send_time_symbol_count as usize, &timing_model)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| anyhow!("Failed to decode timing symbols"))?;
            send_time_data = Some(timing_symbols);
        }

        let mut timing_symbol_position = 0usize;
        for (i, &rtt_symbol) in rtt_symbols.iter().enumerate() {
            let rtt_nanos = if rtt_symbol == PACKET_LOST_SYMBOL {
                u64::MAX
            } else {
                quantizer.symbol_to_duration(rtt_symbol).as_nanos() as u64
            };

            if i > 0 {
                if let Some(ref timing_symbols) = send_time_data {
                    let mut total_interval = 0u64;
                    const QUANTUM_NS: u64 = 100_000;
                    const JUMP_FORWARD_SYMBOL: u16 = 65535;
                    const MAX_SYMBOL_NS: u64 = 6_553_500_000;

                    while timing_symbol_position < timing_symbols.len() {
                        let timing_symbol = timing_symbols[timing_symbol_position];
                        timing_symbol_position += 1;
                        if timing_symbol == JUMP_FORWARD_SYMBOL {
                            total_interval += MAX_SYMBOL_NS;
                        } else {
                            total_interval += timing_symbol as u64 * QUANTUM_NS;
                            break;
                        }
                    }
                    current_sent_nanos += total_interval;
                } else {
                    current_sent_nanos += chunk_header.base_interval_ns;
                }
            }
            all_records.push(RawDataRecord {
                sent_nanos: current_sent_nanos,
                rtt_nanos,
            });
        }

        let chunk_data_end = rtt_stream_start
            + chunk_header.rtt_stream_len_bytes as usize
            + chunk_header.send_time_stream_len_bytes as usize;
        let expected_crc32_start = chunk_data_end;
        let expected_end_asterisk = expected_crc32_start + 4;

        if expected_end_asterisk >= chunk_end {
            return Err(anyhow!(
                "Chunk {} data extends beyond chunk boundary: {} >= {}",
                chunk_index,
                expected_end_asterisk,
                chunk_end
            ));
        }

        let chunk_data = &data[chunk_offset..chunk_data_end];
        let computed_chunk_crc32 = crc32fast::hash(chunk_data);
        let stored_crc32 = u32::from_be_bytes(
            data[expected_crc32_start..expected_crc32_start + 4]
                .try_into()
                .unwrap(),
        );
        if computed_chunk_crc32 != stored_crc32 {
            return Err(anyhow!(
                "Chunk {} data CRC32 mismatch: expected 0x{:08x}, computed 0x{:08x}",
                chunk_index,
                stored_crc32,
                computed_chunk_crc32
            ));
        }

        if data[expected_end_asterisk] != b'*' {
            return Err(anyhow!(
                "Missing end asterisk delimiter for chunk {} at offset {}",
                chunk_index,
                expected_end_asterisk
            ));
        }
    }

    Ok(all_records)
}
