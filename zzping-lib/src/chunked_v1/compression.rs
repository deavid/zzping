//! Contains the logic for compressing `RawDataRecord`s into the `chunked_v1` format.

use super::format::{
    calculate_chunk_header_crc32, calculate_file_header_crc32, AggregateEntry, ChunkHeader,
    ChunkFlags, FileHeader, FILE_MAGIC, FORMAT_VERSION, HEADER_SIZE,
};
use super::quantization::{Quantizer, PACKET_LOST_SYMBOL, DUMMY_SYMBOL};
use crate::protocol::RawDataRecord;
use anyhow::{anyhow, Result};
use byteorder::{BigEndian, WriteBytesExt};
use constriction::stream::{model::DefaultNonContiguousCategoricalEncoderModel, stack::DefaultAnsCoder};
use std::time::Duration;

/// Calculates statistical aggregates for a slice of `Duration`s.
///
/// This function computes p0, p10, ..., p100 percentiles for the given RTTs.
/// The results are returned as quantized symbols in an `AggregateEntry`.
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
        lost_packet_count: 0, // This is set later, as this function only sees successful RTTs.
    }
}

/// Calculates statistical aggregates for a slice of send-time intervals.
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
        lost_packet_count: 0, // Not applicable to send-time intervals.
    }
}

/// The strategy for encoding ping send times.
enum SendTimeStrategy {
    /// Used when ping send times have a nearly constant interval.
    /// The `base_interval_ns` is stored in the chunk header, and individual
    /// send times are not stored in the payload, saving space.
    ConstantRate { base_interval_ns: u64 },
    /// Used when ping send times are variable.
    /// The deltas between send times are quantized and compressed in the payload.
    QuantizedVariable {
        base_interval_ns: u64,
        timing_symbols: Vec<u16>,
        intervals: Vec<u64>,
    },
}

/// Analyzes the send times of a chunk of records to determine the best encoding strategy.
///
/// If 95% or more of the intervals between pings are within a 1ms tolerance of the
/// median interval, it chooses the `ConstantRate` strategy. Otherwise, it uses
/// `QuantizedVariable`. This heuristic is key to the format's compression efficiency.
fn analyze_send_times(records: &[RawDataRecord]) -> SendTimeStrategy {
    if records.len() < 2 {
        return SendTimeStrategy::ConstantRate {
            base_interval_ns: 0,
        };
    }

    let intervals: Vec<u64> = records
        .windows(2)
        .map(|w| w[1].sent_nanos - w[0].sent_nanos)
        .collect();

    let base_interval_ns = {
        if intervals.is_empty() {
            return SendTimeStrategy::ConstantRate {
                base_interval_ns: 0,
            };
        }
        let mut sorted_intervals = intervals.to_vec();
        sorted_intervals.sort_unstable();
        sorted_intervals[sorted_intervals.len() / 2]
    };

    if base_interval_ns == 0 {
        return SendTimeStrategy::ConstantRate {
            base_interval_ns: 0,
        };
    }

    let mut constant_count = 0;
    let tolerance_ns = 1_000_000; // 1ms

    for &interval in &intervals {
        if (interval as i64 - base_interval_ns as i64).abs() <= tolerance_ns {
            constant_count += 1;
        }
    }

    if constant_count as f64 / intervals.len() as f64 >= 0.95 {
        return SendTimeStrategy::ConstantRate { base_interval_ns };
    }

    let mut timing_symbols = Vec::new();
    const QUANTUM_NS: u64 = 100_000; // 0.1ms
    const MAX_SYMBOL_NS: u64 = 6_553_500_000; // 6.5535s
    const JUMP_FORWARD_SYMBOL: u16 = 65535;

    for &interval in intervals.iter() {
        let mut remaining_ns = interval;
        while remaining_ns > MAX_SYMBOL_NS {
            timing_symbols.push(JUMP_FORWARD_SYMBOL);
            remaining_ns -= MAX_SYMBOL_NS;
        }
        timing_symbols.push((remaining_ns / QUANTUM_NS) as u16);
    }

    SendTimeStrategy::QuantizedVariable {
        base_interval_ns,
        timing_symbols,
        intervals,
    }
}

/// Builds a probability model for the ANS compressor from aggregate statistics.
///
/// Instead of building a frequency table by iterating over all symbols (which can be
/// slow for large chunks), this function approximates the distribution using the
/// pre-calculated percentiles from the `AggregateEntry`. This is a key performance
/// optimization.
pub(super) fn build_model(stats: &AggregateEntry, chunk_symbol_count: usize) -> Result<(Vec<u16>, Vec<f64>)> {
    let mut frequencies = vec![0u32; u16::MAX as usize + 1];
    let valid_symbols_count = chunk_symbol_count - stats.lost_packet_count as usize;

    if valid_symbols_count > 0 {
        let percentile_points = [
            stats.p00_symbol, stats.p10_symbol, stats.p20_symbol, stats.p30_symbol,
            stats.p40_symbol, stats.p50_symbol, stats.p60_symbol, stats.p70_symbol,
            stats.p80_symbol, stats.p90_symbol, stats.p100_symbol,
        ];

        let bucket_count = valid_symbols_count / 10;
        for i in 0..10 {
            let start_symbol = percentile_points[i] as usize;
            let end_symbol = percentile_points[i + 1] as usize;
            let symbol_range_size = (end_symbol - start_symbol) + 1;
            let freq = (bucket_count as f64 / symbol_range_size as f64).ceil() as u32;
            let freq = freq.max(1);
            for freq_s in frequencies.iter_mut().take(end_symbol + 1).skip(start_symbol) {
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
    let probabilities_f64: Vec<f64> = probabilities.iter().map(|&f| f as f64 / total_freq as f64).collect();

    Ok((symbols_with_freq, probabilities_f64))
}

/// Creates an empty file header for a new `.zzp1` file.
///
/// The header is initialized with default values and padded to the full `HEADER_SIZE`.
/// This is intended to be written once when a new daily file is created.
pub fn create_chunked_v1_header() -> Result<Vec<u8>> {
    let mut header_buf = vec![0u8; HEADER_SIZE];
    let mut file_header = FileHeader {
        magic: FILE_MAGIC,
        format_version: FORMAT_VERSION,
        start_time_unix_ns: 0,
        aggregate_entry_count: 0,
        index_entry_count: 0,
        header_crc32: 0,
    };
    file_header.header_crc32 = calculate_file_header_crc32(&file_header);

    let mut cursor = std::io::Cursor::new(&mut header_buf[..]);
    file_header.write(&mut cursor)?;

    Ok(header_buf)
}

/// Compresses a single minute of `RawDataRecord`s into a chunk body.
///
/// This function creates the binary data for a single chunk, including its
/// header, compressed payload, and CRC footer. The resulting `Vec<u8>` is
/// designed to be appended directly to a `.zzp1` file.
pub fn create_chunk_body(chunk_records: &[RawDataRecord]) -> Result<Vec<u8>> {
    if chunk_records.is_empty() {
        return Ok(Vec::new());
    }

    let quantizer = Quantizer::new();
    let valid_rtts: Vec<Duration> = chunk_records
        .iter()
        .filter(|r| r.rtt_nanos != u64::MAX)
        .map(|r| Duration::from_nanos(r.rtt_nanos))
        .collect();

    let mut rtt_stats = calculate_duration_stats(&valid_rtts, &quantizer);
    rtt_stats.lost_packet_count = (chunk_records.len() - valid_rtts.len()) as u32;

    let send_time_strategy = analyze_send_times(chunk_records);

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
            SendTimeStrategy::ConstantRate { .. } => (Vec::new(), 0, None),
            SendTimeStrategy::QuantizedVariable { timing_symbols, intervals, .. } => {
                flags |= ChunkFlags::IS_VARIABLE_RATE;
                let send_time_stats = calculate_interval_stats(intervals);
                let mut timing_frequencies = vec![0u32; u16::MAX as usize + 1];
                for &symbol in timing_symbols {
                    timing_frequencies[symbol as usize] += 1;
                }
                let (timing_symbols_vec, timing_probabilities): (Vec<_>, Vec<_>) =
                    timing_frequencies.iter().enumerate().filter(|&(_, f)| *f > 0).map(|(s, f)| (s as u16, *f)).unzip();
                let total_freq: u32 = timing_probabilities.iter().sum();
                let timing_probabilities_f64: Vec<f64> = timing_probabilities.iter().map(|&f| f as f64 / total_freq as f64).collect();
                let timing_encoded_data = if !timing_symbols_vec.is_empty() {
                    let timing_model = DefaultNonContiguousCategoricalEncoderModel::from_symbols_and_floating_point_probabilities_fast(timing_symbols_vec.clone(), &timing_probabilities_f64, None).map_err(|_| anyhow!("Failed to create timing model"))?;
                    let mut timing_encoder = DefaultAnsCoder::new();
                    timing_encoder.encode_iid_symbols_reverse(timing_symbols, &timing_model)?;
                    timing_encoder.into_compressed()?.iter().flat_map(|w| w.to_be_bytes()).collect()
                } else {
                    Vec::new()
                };

                let mut data = Vec::new();
                data.write_u32::<BigEndian>(timing_symbols_vec.len() as u32)?;
                for &symbol in &timing_symbols_vec {
                    data.write_u16::<BigEndian>(symbol)?;
                }
                for &prob in &timing_probabilities {
                    data.write_u32::<BigEndian>(prob)?;
                }
                data.write_u32::<BigEndian>(timing_encoded_data.len() as u32)?;
                data.extend_from_slice(&timing_encoded_data);

                (data, timing_symbols.len() as u32, Some(send_time_stats))
            }
        };

    let (rtt_s, rtt_p) = build_model(&rtt_stats, rtt_symbols.len())?;
    let rtt_encoded_data = if !rtt_s.is_empty() {
        let rtt_model = DefaultNonContiguousCategoricalEncoderModel::from_symbols_and_floating_point_probabilities_fast(rtt_s, &rtt_p, None).map_err(|_| anyhow!("Failed to create categorical model"))?;
        let mut rtt_encoder = DefaultAnsCoder::new();
        rtt_encoder.encode_iid_symbols_reverse(&rtt_symbols, &rtt_model)?;
        rtt_encoder.into_compressed()?.iter().flat_map(|w| w.to_be_bytes()).collect()
    } else {
        Vec::new()
    };

    let first_ping_time = chunk_records.first().map_or(0, |r| r.sent_nanos);
    let minute_boundary_unix_ns = (first_ping_time / 60_000_000_000) * 60_000_000_000;
    let first_ping_offset_ns = first_ping_time - minute_boundary_unix_ns;

    let base_interval_ns = match &send_time_strategy {
        SendTimeStrategy::ConstantRate { base_interval_ns } => *base_interval_ns,
        SendTimeStrategy::QuantizedVariable { base_interval_ns, .. } => *base_interval_ns,
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
        chunk_crc32: 0,
    };
    chunk_header.chunk_crc32 = calculate_chunk_header_crc32(&chunk_header);

    let mut chunk_header_buffer = Vec::new();
    chunk_header.write(&mut chunk_header_buffer)?;

    let mut full_chunk_buffer = Vec::new();
    full_chunk_buffer.extend_from_slice(&chunk_header_buffer);
    full_chunk_buffer.extend_from_slice(&rtt_encoded_data);
    full_chunk_buffer.extend_from_slice(&send_time_encoded_data);

    let asterisk_delimiter = b"*";
    let chunk_data_crc32 = crc32fast::hash(&full_chunk_buffer);

    let mut final_chunk_data = Vec::new();
    final_chunk_data.extend_from_slice(asterisk_delimiter);
    final_chunk_data.extend_from_slice(&full_chunk_buffer);
    final_chunk_data.extend_from_slice(&chunk_data_crc32.to_be_bytes());
    final_chunk_data.extend_from_slice(asterisk_delimiter);

    Ok(final_chunk_data)
}
