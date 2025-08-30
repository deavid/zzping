use super::format::{
    calculate_chunk_header_crc32, calculate_file_header_crc32, AggregateEntry, ChunkHeader,
    ChunkFlags, FileHeader, IndexEntry, FILE_MAGIC, FORMAT_VERSION, HEADER_SIZE,
    MAX_RECOMMENDED_CHUNKS,
};
use super::quantization::{Quantizer, PACKET_LOST_SYMBOL, DUMMY_SYMBOL};
use crate::protocol::RawDataRecord;
use anyhow::{anyhow, Result};
use byteorder::{BigEndian, WriteBytesExt};
use constriction::stream::{model::DefaultNonContiguousCategoricalEncoderModel, stack::DefaultAnsCoder};
use std::time::Duration;

fn calculate_duration_stats(durations: &[Duration], quantizer: &Quantizer) -> AggregateEntry {
    if durations.is_empty() {
        return AggregateEntry::default();
    }

    let mut sorted_durations = durations.to_vec();
    sorted_durations.sort();

    let n = sorted_durations.len() - 1;
    let get_percentile = |p: usize| -> u16 {
        let index = (p as f64 / 100.0 * n as f64).round() as usize;
        quantizer.duration_to_symbol(sorted_durations[index])
    };

    AggregateEntry {
        p00_symbol: get_percentile(0),
        p10_symbol: get_percentile(10),
        p20_symbol: get_percentile(20),
        p30_symbol: get_percentile(30),
        p40_symbol: get_percentile(40),
        p50_symbol: get_percentile(50),
        p60_symbol: get_percentile(60),
        p70_symbol: get_percentile(70),
        p80_symbol: get_percentile(80),
        p90_symbol: get_percentile(90),
        p100_symbol: get_percentile(100),
        lost_packet_count: 0, // Will be set separately for RTTs.
    }
}

fn calculate_interval_stats(intervals: &[u64]) -> AggregateEntry {
    if intervals.is_empty() {
        return AggregateEntry::default();
    }

    let mut sorted_intervals = intervals.to_vec();
    sorted_intervals.sort();

    const QUANTUM_NS: u64 = 100_000; // 0.1ms quantum

    let n = sorted_intervals.len() - 1;
    let get_percentile = |p: usize| -> u16 {
        let index = (p as f64 / 100.0 * n as f64).round() as usize;
        // Direct quantization for intervals, no jump tokens needed for stats
        (sorted_intervals[index] / QUANTUM_NS) as u16
    };

    AggregateEntry {
        p00_symbol: get_percentile(0),
        p10_symbol: get_percentile(10),
        p20_symbol: get_percentile(20),
        p30_symbol: get_percentile(30),
        p40_symbol: get_percentile(40),
        p50_symbol: get_percentile(50),
        p60_symbol: get_percentile(60),
        p70_symbol: get_percentile(70),
        p80_symbol: get_percentile(80),
        p90_symbol: get_percentile(90),
        p100_symbol: get_percentile(100),
        lost_packet_count: 0, // Not applicable to intervals
    }
}

// TODO: Add tests for this timing strategy selection logic
// TODO: Test edge cases: single record chunks, identical timestamps, extreme jitter
// TODO: Verify minute boundary alignment works correctly across timezones and DST
// TODO: Add test for quantization precision: ensure 1ns quantum doesn't cause overflow
enum SendTimeStrategy {
    ConstantRate {
        base_interval_ns: u64,
    },
    QuantizedVariable {
        base_interval_ns: u64,
        timing_symbols: Vec<u16>, // 0.1ms quantized intervals + jump tokens
        intervals: Vec<u64>,      // Raw intervals for stats calculation
    },
}

// Helper function to calculate the most common interval (mode)
fn calculate_mode_interval(intervals: &[u64]) -> u64 {
    if intervals.is_empty() {
        return 1_000_000_000; // Default 1 second
    }

    // Use median as an approximation of the mode for performance
    // This is faster than calculating true mode for large datasets
    let mut sorted_intervals = intervals.to_vec();
    sorted_intervals.sort_unstable();
    sorted_intervals[sorted_intervals.len() / 2]
}

fn analyze_send_times(records: &[RawDataRecord]) -> SendTimeStrategy {
    if records.len() < 2 {
        return SendTimeStrategy::ConstantRate {
            base_interval_ns: 0,
        };
    }

    // Calculate all intervals between consecutive records
    let intervals: Vec<u64> = records
        .windows(2)
        .map(|w| w[1].sent_nanos - w[0].sent_nanos)
        .collect();

    let base_interval_ns = calculate_mode_interval(&intervals);

    if base_interval_ns == 0 {
        // All records have same timestamp - treat as constant with 0 interval
        return SendTimeStrategy::ConstantRate {
            base_interval_ns: 0,
        };
    }

    // Check if intervals are truly constant (within a small tolerance)
    let mut constant_count = 0;
    let tolerance_ns = 1_000_000; // 1ms tolerance

    for &interval in &intervals {
        let deviation = (interval as i64 - base_interval_ns as i64).abs();
        if deviation <= tolerance_ns {
            constant_count += 1;
        }
    }

    let constant_ratio = constant_count as f64 / intervals.len() as f64;

    // If 95% or more intervals are constant within tolerance, use ConstantRate
    if constant_ratio >= 0.95 {
        return SendTimeStrategy::ConstantRate { base_interval_ns };
    }

    // Use quantized variable rate for variable timing
    let mut timing_symbols = Vec::new();

    const QUANTUM_NS: u64 = 100_000; // 0.1ms quantum
    const MAX_SYMBOL_NS: u64 = 6_553_500_000; // 6.5535s (65535 * 0.1ms)
    const JUMP_FORWARD_SYMBOL: u16 = 65535; // Jump forward 6.5535s

    for &interval in intervals.iter() {
        let mut remaining_ns = interval;

        // Emit jump tokens for intervals > 6.5535s
        while remaining_ns > MAX_SYMBOL_NS {
            timing_symbols.push(JUMP_FORWARD_SYMBOL);
            remaining_ns -= MAX_SYMBOL_NS;
        }

        // Emit the final symbol for the remaining interval
        let quantized_symbol = (remaining_ns / QUANTUM_NS) as u16;
        timing_symbols.push(quantized_symbol);
    }

    SendTimeStrategy::QuantizedVariable {
        base_interval_ns,
        timing_symbols,
        intervals,
    }
}

pub(super) fn build_model(stats: &AggregateEntry, chunk_symbol_count: usize) -> Result<(Vec<u16>, Vec<f64>)> {
    let mut frequencies = vec![0u32; u16::MAX as usize + 1];
    let valid_symbols_count = chunk_symbol_count - stats.lost_packet_count as usize;

    if valid_symbols_count > 0 {
        let percentile_points = [
            stats.p00_symbol,
            stats.p10_symbol,
            stats.p20_symbol,
            stats.p30_symbol,
            stats.p40_symbol,
            stats.p50_symbol,
            stats.p60_symbol,
            stats.p70_symbol,
            stats.p80_symbol,
            stats.p90_symbol,
            stats.p100_symbol,
        ];

        let bucket_count = valid_symbols_count / 10;
        for i in 0..10 {
            let start_symbol = percentile_points[i] as usize;
            let end_symbol = percentile_points[i + 1] as usize;
            let symbol_range_size = (end_symbol - start_symbol) + 1;
            let freq = (bucket_count as f64 / symbol_range_size as f64).ceil() as u32;
            let freq = freq.max(1);
            for freq_s in frequencies
                .iter_mut()
                .take(end_symbol + 1)
                .skip(start_symbol)
            {
                *freq_s = freq;
            }
        }
    }

    frequencies[PACKET_LOST_SYMBOL as usize] = stats.lost_packet_count.max(1);
    frequencies[DUMMY_SYMBOL as usize] = 1;

    let (symbols_with_freq, probabilities): (Vec<_>, Vec<_>) = frequencies
        .iter()
        .enumerate()
        .filter(|&(_, f)| *f > 0)
        .map(|(s, f)| (s as u16, *f))
        .unzip();

    let total_freq: u32 = probabilities.iter().sum();

    let probabilities_f64: Vec<f64> = probabilities
        .iter()
        .map(|&f| f as f64 / total_freq as f64)
        .collect();

    Ok((symbols_with_freq, probabilities_f64))
}

// Compress ping data using the chunked v1 format.
//
// DESIGN LIMITATION: This format is intended for 24-hour data collection periods.
// The 64KiB header can accommodate approximately 1440 chunks (one per minute for 24 hours).
// Data spanning multiple days will fail with "failed to write whole buffer" due to header overflow.
// For multi-day datasets, split data by day and compress each day separately.
//
// TODO: Add comprehensive tests for this compression function covering:
// - Empty input (handled)
// - Single record chunks
// - Large datasets (GB-sized)
// - All packet loss scenarios
// - Clock anomalies (backward time, leap seconds)
// - Memory usage under stress
// - Compression ratio benchmarks vs expectations
pub fn compress_chunked_v1(records: &[RawDataRecord]) -> Result<Vec<u8>> {
    if records.is_empty() {
        return Ok(Vec::new());
    }

    let quantizer = Quantizer::new();

    let mut chunks: std::collections::BTreeMap<u64, Vec<RawDataRecord>> =
        std::collections::BTreeMap::new();

    for record in records {
        let minute_index = record.sent_nanos / (60 * 1_000_000_000);
        chunks.entry(minute_index).or_default().push(*record);
    }

    // Check for format design limitation: too many chunks for 24-hour format
    if chunks.len() > MAX_RECOMMENDED_CHUNKS {
        return Err(anyhow!(
            "Format limitation exceeded: {} chunks requested, but format designed for max {} chunks (24 hours). \
             For multi-day data, split into separate files by day.",
            chunks.len(),
            MAX_RECOMMENDED_CHUNKS
        ));
    }

    let mut payload_buffer = Vec::new();
    let mut aggregate_entries = Vec::new();
    let mut index_entries = Vec::new();

    let mut total_rtt_bytes = 0;
    let mut total_timing_bytes = 0;
    let mut constant_rate_chunks = 0;
    let mut variable_rate_chunks = 0;

    for (_minute_index, chunk_records) in chunks {
        let valid_rtts: Vec<Duration> = chunk_records
            .iter()
            .filter(|r| r.rtt_nanos != u64::MAX)
            .map(|r| Duration::from_nanos(r.rtt_nanos))
            .collect();

        let mut rtt_stats = calculate_duration_stats(&valid_rtts, &quantizer);
        rtt_stats.lost_packet_count = (chunk_records.len() - valid_rtts.len()) as u32;
        aggregate_entries.push(rtt_stats);

        let send_time_strategy = analyze_send_times(&chunk_records);

        let rtt_symbols: Vec<u16> = chunk_records
            .iter()
            .map(|r| {
                if r.rtt_nanos == u64::MAX {
                    PACKET_LOST_SYMBOL
                } else {
                    quantizer.duration_to_symbol(Duration::from_nanos(r.rtt_nanos))
                }
            })
            .collect();

        let mut flags = ChunkFlags::empty();

        let (send_time_encoded_data, send_time_symbol_count, send_time_stats) =
            match &send_time_strategy {
                SendTimeStrategy::ConstantRate {
                    base_interval_ns: _,
                } => {
                    constant_rate_chunks += 1;
                    (Vec::new(), 0, None)
                }
                SendTimeStrategy::QuantizedVariable {
                    base_interval_ns: _,
                    timing_symbols,
                    intervals,
                } => {
                    variable_rate_chunks += 1;
                    flags |= ChunkFlags::IS_VARIABLE_RATE;

                    let send_time_stats = calculate_interval_stats(intervals);

                    // Build frequency table for timing symbols
                    let mut timing_frequencies = vec![0u32; u16::MAX as usize + 1];
                    for &symbol in timing_symbols {
                        timing_frequencies[symbol as usize] += 1;
                    }

                    // Build ANS model for timing symbols
                    let (timing_symbols_vec, timing_probabilities): (Vec<_>, Vec<_>) =
                        timing_frequencies
                            .iter()
                            .enumerate()
                            .filter(|&(_, f)| *f > 0)
                            .map(|(s, f)| (s as u16, *f))
                            .unzip();

                    let total_freq: u32 = timing_probabilities.iter().sum();
                    let timing_probabilities_f64: Vec<f64> = timing_probabilities
                        .iter()
                        .map(|&f| f as f64 / total_freq as f64)
                        .collect();

                    // Encode timing symbols using ANS
                    let timing_encoded_data = if !timing_symbols_vec.is_empty() {
                        let timing_model = DefaultNonContiguousCategoricalEncoderModel::from_symbols_and_floating_point_probabilities_fast(timing_symbols_vec.clone(), &timing_probabilities_f64, None)
                            .map_err(|_| anyhow!("Failed to create timing model"))?;
                        let mut timing_encoder = DefaultAnsCoder::new();
                        timing_encoder.encode_iid_symbols_reverse(timing_symbols, &timing_model)?;
                        let timing_encoded_data_u32 = timing_encoder.into_compressed()?;
                        let data: Vec<u8> = timing_encoded_data_u32
                            .iter()
                            .flat_map(|w| w.to_be_bytes())
                            .collect();
                        data
                    } else {
                        Vec::new()
                    };

                    let mut data = Vec::new();

                    // Store timing model (symbols and probabilities)
                    data.write_u32::<BigEndian>(timing_symbols_vec.len() as u32)?;
                    for &symbol in &timing_symbols_vec {
                        data.write_u16::<BigEndian>(symbol)?;
                    }
                    for &prob in &timing_probabilities {
                        data.write_u32::<BigEndian>(prob)?;
                    }

                    // Store compressed timing data
                    data.write_u32::<BigEndian>(timing_encoded_data.len() as u32)?;
                    data.extend_from_slice(&timing_encoded_data);

                    total_timing_bytes += data.len();

                    (data, timing_symbols.len() as u32, Some(send_time_stats))
                }
            };

        let (rtt_s, rtt_p) = build_model(&rtt_stats, rtt_symbols.len())?;
        let rtt_encoded_data = if !rtt_s.is_empty() {
            let rtt_model = DefaultNonContiguousCategoricalEncoderModel::from_symbols_and_floating_point_probabilities_fast(rtt_s, &rtt_p, None)
                .map_err(|_| anyhow!("Failed to create categorical model"))?;
            let mut rtt_encoder = DefaultAnsCoder::new();
            rtt_encoder.encode_iid_symbols_reverse(&rtt_symbols, &rtt_model)?;
            let rtt_encoded_data_u32 = rtt_encoder.into_compressed()?;
            let data: Vec<u8> = rtt_encoded_data_u32
                .iter()
                .flat_map(|w| w.to_be_bytes())
                .collect();
            total_rtt_bytes += data.len();
            data
        } else {
            Vec::new()
        };

        // Calculate minute boundary and offset for first ping
        let first_ping_time = chunk_records.first().map_or(0, |r| r.sent_nanos);
        let minute_boundary_unix_ns =
            (first_ping_time / (60 * 1_000_000_000)) * (60 * 1_000_000_000);
        let first_ping_offset_ns = first_ping_time - minute_boundary_unix_ns;

        // Extract base interval from strategy
        let base_interval_ns = match &send_time_strategy {
            SendTimeStrategy::ConstantRate { base_interval_ns } => *base_interval_ns,
            SendTimeStrategy::QuantizedVariable {
                base_interval_ns, ..
            } => *base_interval_ns,
        };

        let mut chunk_header = ChunkHeader {
            minute_boundary_unix_ns,
            first_ping_offset_ns,
            rtt_symbol_count: rtt_symbols.len() as u32,
            send_time_symbol_count,
            rtt_stream_len_bytes: rtt_encoded_data.len() as u32,
            send_time_stream_len_bytes: send_time_encoded_data.len() as u32,
            flags,
            base_interval_ns,
            rtt_stats,
            send_time_stats,
            chunk_crc32: 0, // Will be calculated below
        };

        // Calculate and set the chunk header CRC32 (Phase 2)
        chunk_header.chunk_crc32 = calculate_chunk_header_crc32(&chunk_header);

        let mut chunk_buffer = Vec::new();
        chunk_header.write(&mut chunk_buffer)?;
        chunk_buffer.extend_from_slice(&rtt_encoded_data);
        chunk_buffer.extend_from_slice(&send_time_encoded_data);

        // Phase 4: Add asterisk delimiters and chunk CRC32
        let asterisk_delimiter = b"*"; // Single asterisk byte as chunk delimiter
        let chunk_data_crc32 = crc32fast::hash(&chunk_buffer);

        index_entries.push(IndexEntry {
            chunk_offset_bytes: (HEADER_SIZE + payload_buffer.len() + asterisk_delimiter.len())
                as u64,
        });

        // Write: *[chunk_data][crc32]*
        payload_buffer.extend_from_slice(asterisk_delimiter); // Start delimiter
        payload_buffer.extend_from_slice(&chunk_buffer);
        payload_buffer.extend_from_slice(&chunk_data_crc32.to_be_bytes()); // CRC32 of chunk data
        payload_buffer.extend_from_slice(asterisk_delimiter); // End delimiter
    }

    // --- 3. Finalize the file ---
    let mut header_buf = vec![0u8; HEADER_SIZE];

    let mut file_header = FileHeader {
        magic: FILE_MAGIC,
        format_version: FORMAT_VERSION,
        start_time_unix_ns: records.first().map_or(0, |r| r.sent_nanos),
        aggregate_entry_count: aggregate_entries.len() as u32,
        index_entry_count: index_entries.len() as u32,
        header_crc32: 0, // Will be calculated below
    };

    // Calculate and set the file header CRC32 (Phase 2)
    file_header.header_crc32 = calculate_file_header_crc32(&file_header);

    let mut cursor = std::io::Cursor::new(&mut header_buf[..]);
    file_header.write(&mut cursor)?;

    for entry in &aggregate_entries {
        entry.write(&mut cursor)?;
    }

    for entry in &index_entries {
        entry.write(&mut cursor)?;
    }

    let mut final_data = header_buf;
    final_data.extend_from_slice(&payload_buffer);

    // Print compression debug summary only if explicitly requested
    if std::env::var("ZZPING_DEBUG_COMPRESSION").is_ok() {
        println!("=== COMPRESSION DEBUG SUMMARY ===");
        println!(
            "Total chunks: {} ({} constant rate, {} variable rate)",
            constant_rate_chunks + variable_rate_chunks,
            constant_rate_chunks,
            variable_rate_chunks
        );
        println!("RTT data: {total_rtt_bytes} bytes");
        println!("Timing data: {total_timing_bytes} bytes");
        println!("Header size: {HEADER_SIZE} bytes");
        println!("Payload size: {} bytes", payload_buffer.len());
        println!("Total size: {} bytes", final_data.len());

        if constant_rate_chunks > 0 && variable_rate_chunks > 0 {
            println!(
                "WARNING: Mixed chunk types detected! {} constant, {} variable",
                constant_rate_chunks, variable_rate_chunks
            );
        } else if variable_rate_chunks > 0 {
            println!("All chunks are variable rate - constant rate optimization not being used!");
        } else {
            println!("All chunks are constant rate - timing optimization working correctly");
        }
        println!("==================================");
    }

    Ok(final_data)
}
