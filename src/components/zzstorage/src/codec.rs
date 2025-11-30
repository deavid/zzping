// src/components/zzstorage/src/codec.rs

//! This module contains the v2 data format, codec, and compression logic.
use anyhow::Result;
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use constriction::stream::{
    model::{DefaultNonContiguousCategoricalEncoderModel, NonContiguousCategoricalDecoderModel},
    stack::DefaultAnsCoder,
    Decode,
};
use std::collections::HashMap;
use std::io::{Cursor, Read};

/// The status of a single ping attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PingStatus {
    /// The ping was successful and received a reply.
    /// The value is the round-trip-time in nanoseconds.
    Success(u64),
    /// The ping timed out without receiving a reply.
    Timeout,
    /// An IO error occurred during the ping.
    /// This can indicate things like "Network Unreachable" or "Permission Denied".
    IOError,
    /// An "orphaned" record, where the `sent` an event was recorded,
    /// but no corresponding `result` event was received before the batch was flushed.
    /// This is typically generated on the Database side.
    Partial,
    /// The ping interval was skipped for flow control reasons (e.g., backpressure).
    Skipped,
    /// A catch-all for any other status.
    Other,
}

/// The result of a single ping measurement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PingResult {
    /// The target that was pinged (e.g., "8.8.8.8").
    pub target: String,
    /// The UNIX timestamp in nanoseconds when the ping was sent.
    pub sent_time_ns: u64,
    /// The outcome of the ping.
    pub status: PingStatus,
}

/// Represents a batch of ping results, grouped by target.
/// This is the input to the compression function.
pub type PingBatch = Vec<PingResult>;

/// The strategy for encoding ping send times.
enum SendTimeStrategy {
    ConstantRate {
        base_interval_ns: u64,
    },
    VariableRate {
        timing_symbols: Vec<u16>,
    },
}

/// Compresses a batch of `PingResult`s into a compressed blob.
pub fn compress_batch(batch: &PingBatch) -> Result<Vec<u8>> {
    if batch.is_empty() {
        return Ok(Vec::new());
    }

    let mut records_by_target: HashMap<String, Vec<&PingResult>> = HashMap::new();
    for record in batch {
        records_by_target
            .entry(record.target.clone())
            .or_default()
            .push(record);
    }

    let mut compressed_blob = Vec::new();
    for (target, records) in records_by_target {
        compressed_blob.write_u16::<BigEndian>(target.len() as u16)?;
        compressed_blob.extend(target.as_bytes());
        let target_data = compress_target_data(&records)?;
        compressed_blob.extend(&target_data);
    }
    Ok(compressed_blob)
}

/// Decompresses a blob of data into a `PingBatch`.
pub fn decompress_batch(data: &[u8]) -> Result<PingBatch> {
    let mut batch = PingBatch::new();
    let mut cursor = Cursor::new(data);

    while (cursor.position() as usize) < data.len() {
        let target_len = cursor.read_u16::<BigEndian>()? as usize;
        let mut target_buf = vec![0; target_len];
        cursor.read_exact(&mut target_buf)?;
        let target = String::from_utf8(target_buf)?;

        let (sent_times, statuses) = decompress_target_data(&mut cursor)?;

        for (sent_time_ns, status) in sent_times.into_iter().zip(statuses.into_iter()) {
            batch.push(PingResult {
                target: target.clone(),
                sent_time_ns,
                status,
            });
        }
    }
    Ok(batch)
}

/// Analyzes the send times of a chunk of records to determine the best encoding strategy.
fn analyze_send_times(records: &[&PingResult]) -> SendTimeStrategy {
    if records.len() < 2 {
        return SendTimeStrategy::ConstantRate {
            base_interval_ns: 0,
        };
    }

    let intervals: Vec<u64> = records
        .windows(2)
        .map(|w| w[1].sent_time_ns - w[0].sent_time_ns)
        .collect();

    let mut sorted_intervals = intervals.to_vec();
    sorted_intervals.sort_unstable();
    let base_interval_ns = sorted_intervals[sorted_intervals.len() / 2];

    if base_interval_ns == 0 {
        return SendTimeStrategy::ConstantRate {
            base_interval_ns: 0,
        };
    }

    let tolerance_ns = 1_000_000; // 1ms
    let constant_count = intervals
        .iter()
        .filter(|&&interval| (interval as i64 - base_interval_ns as i64).abs() <= tolerance_ns)
        .count();

    if (constant_count as f64 / intervals.len() as f64) >= 0.95 {
        return SendTimeStrategy::ConstantRate { base_interval_ns };
    }

    const QUANTUM_NS: u64 = 100_000; // 0.1ms
    let timing_symbols = intervals
        .iter()
        .map(|&interval| (interval / QUANTUM_NS) as u16)
        .collect();

    SendTimeStrategy::VariableRate { timing_symbols }
}

/// Compresses the data for a single target.
fn compress_target_data(records: &[&PingResult]) -> Result<Vec<u8>> {
    let quantizer = Quantizer::new();

    // 1. Compress Timestamps
    let time_strategy = analyze_send_times(records);
    let (time_header, time_data) = match time_strategy {
        SendTimeStrategy::ConstantRate { base_interval_ns } => {
            let mut header = vec![0u8]; // Strategy 0: Constant
            header.write_u64::<BigEndian>(base_interval_ns)?;
            (header, vec![])
        }
        SendTimeStrategy::VariableRate { timing_symbols } => {
            let (symbols, probs) = build_rtt_model(&timing_symbols)?;
            let encoded_data = if !symbols.is_empty() {
                let model = DefaultNonContiguousCategoricalEncoderModel::from_symbols_and_floating_point_probabilities_fast(
                    symbols.clone(), &probs, None
                ).map_err(|()| anyhow::anyhow!("Failed to create time model"))?;
                let mut encoder = DefaultAnsCoder::new();
                encoder.encode_iid_symbols_reverse(&timing_symbols, &model)?;
                encoder.into_compressed()?.iter().flat_map(|w| w.to_be_bytes()).collect()
            } else {
                vec![]
            };

            let mut header = vec![1u8]; // Strategy 1: Variable
            header.write_u32::<BigEndian>(symbols.len() as u32)?;
            for symbol in symbols { header.write_u16::<BigEndian>(symbol)?; }
            header.write_u32::<BigEndian>(probs.len() as u32)?;
            for prob in probs { header.write_f64::<BigEndian>(prob)?; }
            header.write_u32::<BigEndian>(encoded_data.len() as u32)?;
            (header, encoded_data)
        }
    };

    // 2. Compress RTTs
    let rtt_symbols: Vec<u16> = records
        .iter()
        .map(|r| quantizer.status_to_symbol(&r.status))
        .collect();

    let (model_symbols, model_probs) = build_rtt_model(&rtt_symbols)?;
    let rtt_encoded_data = if !model_symbols.is_empty() {
        let rtt_model = DefaultNonContiguousCategoricalEncoderModel::from_symbols_and_floating_point_probabilities_fast(
            model_symbols.clone(),
            &model_probs,
            None,
        ).map_err(|()| anyhow::anyhow!("Failed to create categorical model"))?;
        let mut rtt_encoder = DefaultAnsCoder::new();
        rtt_encoder.encode_iid_symbols_reverse(&rtt_symbols, &rtt_model)?;
        rtt_encoder.into_compressed()?.iter().flat_map(|w| w.to_be_bytes()).collect()
    } else {
        Vec::new()
    };

    let mut data = Vec::new();
    data.write_u32::<BigEndian>(records.len() as u32)?;
    data.write_u64::<BigEndian>(records.first().map_or(0, |r| r.sent_time_ns))?;

    data.extend(&time_header);
    data.extend(&time_data);

    data.write_u32::<BigEndian>(model_symbols.len() as u32)?;
    for symbol in model_symbols { data.write_u16::<BigEndian>(symbol)?; }
    data.write_u32::<BigEndian>(model_probs.len() as u32)?;
    for prob in model_probs { data.write_f64::<BigEndian>(prob)?; }
    data.write_u32::<BigEndian>(rtt_encoded_data.len() as u32)?;
    data.extend(&rtt_encoded_data);

    Ok(data)
}

fn decompress_target_data(cursor: &mut Cursor<&[u8]>) -> Result<(Vec<u64>, Vec<PingStatus>)> {
    let quantizer = Quantizer::new();
    let num_records = cursor.read_u32::<BigEndian>()? as usize;
    let first_sent_time = cursor.read_u64::<BigEndian>()?;

    // 1. Decompress Timestamps
    let time_strategy = cursor.read_u8()?;
    let mut sent_times = Vec::with_capacity(num_records);
    sent_times.push(first_sent_time);

    match time_strategy {
        0 => { // ConstantRate
            let base_interval_ns = cursor.read_u64::<BigEndian>()?;
            for i in 1..num_records {
                sent_times.push(sent_times[i-1] + base_interval_ns);
            }
        },
        1 => { // VariableRate
            let num_symbols = cursor.read_u32::<BigEndian>()? as usize;
            let mut model_symbols = Vec::with_capacity(num_symbols);
            for _ in 0..num_symbols { model_symbols.push(cursor.read_u16::<BigEndian>()?); }
            let num_probs = cursor.read_u32::<BigEndian>()? as usize;
            let mut model_probs = Vec::with_capacity(num_probs);
            for _ in 0..num_probs { model_probs.push(cursor.read_f64::<BigEndian>()?); }
            let data_len = cursor.read_u32::<BigEndian>()? as usize;
            let pos = cursor.position() as usize;
            let data = &cursor.get_ref()[pos..pos + data_len];
            cursor.set_position((pos + data_len) as u64);

            let timing_symbols = if !model_symbols.is_empty() {
                let model: NonContiguousCategoricalDecoderModel<u16, u32, _, 24> = NonContiguousCategoricalDecoderModel::from_symbols_and_floating_point_probabilities_fast(
                    model_symbols, &model_probs, None
                ).map_err(|()| anyhow::anyhow!("Failed to create time decoder model"))?;
                let compressed_words: Vec<u32> = data.chunks_exact(4).map(|c| u32::from_be_bytes(c.try_into().unwrap())).collect();
                if compressed_words.is_empty() { vec![] } else {
                    let mut decoder = DefaultAnsCoder::from_compressed(compressed_words).map_err(|e| anyhow::anyhow!("Failed to create time decoder: {:?}", e))?;
                    decoder.decode_iid_symbols(num_records - 1, &model).collect::<Result<Vec<_>,_>>()?
                }
            } else { vec![] };

            const QUANTUM_NS: u64 = 100_000; // 0.1ms
            for (i, &symbol) in timing_symbols.iter().enumerate() {
                let interval = symbol as u64 * QUANTUM_NS;
                sent_times.push(sent_times[i] + interval);
            }
        },
        _ => return Err(anyhow::anyhow!("Unknown time strategy")),
    }

    // 2. Decompress RTTs
    let num_symbols = cursor.read_u32::<BigEndian>()? as usize;
    let mut model_symbols = Vec::with_capacity(num_symbols);
    for _ in 0..num_symbols { model_symbols.push(cursor.read_u16::<BigEndian>()?); }
    let num_probs = cursor.read_u32::<BigEndian>()? as usize;
    let mut model_probs = Vec::with_capacity(num_probs);
    for _ in 0..num_probs { model_probs.push(cursor.read_f64::<BigEndian>()?); }
    let data_len = cursor.read_u32::<BigEndian>()? as usize;
    let pos = cursor.position() as usize;
    let data = &cursor.get_ref()[pos..pos + data_len];
    cursor.set_position((pos + data_len) as u64);

    let rtt_symbols = if !model_symbols.is_empty() {
        let rtt_model: NonContiguousCategoricalDecoderModel<u16, u32, _, 24> =
            NonContiguousCategoricalDecoderModel::from_symbols_and_floating_point_probabilities_fast(
                model_symbols, &model_probs, None
            ).map_err(|()| anyhow::anyhow!("Failed to create categorical decoder model"))?;
        let compressed_words: Vec<u32> = data.chunks_exact(4).map(|c| u32::from_be_bytes(c.try_into().unwrap())).collect();
        if compressed_words.is_empty() { vec![] } else {
            let mut decoder = DefaultAnsCoder::from_compressed(compressed_words).map_err(|e| anyhow::anyhow!("Failed to create decoder: {:?}", e))?;
            decoder.decode_iid_symbols(num_records, &rtt_model).collect::<Result<Vec<_>,_>>()?
        }
    } else { vec![] };

    let statuses = rtt_symbols.iter().map(|&s| quantizer.symbol_to_status(s)).collect();
    Ok((sent_times, statuses))
}


fn build_rtt_model(rtt_symbols: &[u16]) -> Result<(Vec<u16>, Vec<f64>)> {
    if rtt_symbols.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let mut frequencies = HashMap::new();
    for &symbol in rtt_symbols {
        *frequencies.entry(symbol).or_insert(0) += 1;
    }

    if frequencies.len() == 1 {
        let present_symbol = *frequencies.keys().next().unwrap();
        let dummy_symbol = if present_symbol == 0 { 1 } else { 0 };
        frequencies.insert(dummy_symbol, 1);
    }

    let total_freq: u32 = frequencies.values().sum();
    let (symbols, probabilities): (Vec<_>, Vec<_>) = frequencies
        .into_iter()
        .map(|(s, f)| (s, f as f64 / total_freq as f64))
        .unzip();

    Ok((symbols, probabilities))
}

/// A quantizer for converting between nanosecond RTTs and symbols.
#[derive(Debug, Clone, Copy)]
pub struct Quantizer;

impl Quantizer {
    /// Creates a new `Quantizer`.
    pub fn new() -> Self {
        Self
    }

    /// Converts a `PingStatus` to a 16-bit symbol for compression.
    pub fn status_to_symbol(&self, status: &PingStatus) -> u16 {
        match status {
            PingStatus::Success(ns) => self.duration_ns_to_symbol(*ns),
            PingStatus::Timeout => 65530,
            PingStatus::IOError => 65531,
            PingStatus::Partial => 65532,
            PingStatus::Skipped => 65533,
            PingStatus::Other => 65534,
        }
    }

    /// Converts a 16-bit symbol back to a `PingStatus`.
    pub fn symbol_to_status(&self, symbol: u16) -> PingStatus {
        match symbol {
            s if s <= 460 => PingStatus::Success(self.symbol_to_duration_ns(s)),
            65530 => PingStatus::Timeout,
            65531 => PingStatus::IOError,
            65532 => PingStatus::Partial,
            65533 => PingStatus::Skipped,
            _ => PingStatus::Other,
        }
    }

    /// Converts a duration in nanoseconds to a 16-bit symbol.
    pub fn duration_ns_to_symbol(&self, nanos: u64) -> u16 {
        const MICROSECOND: u64 = 1_000;
        const MILLISECOND: u64 = 1_000_000;
        const SECOND: u64 = 1_000_000_000;

        match nanos {
            0..=MILLISECOND => (nanos / (10 * MICROSECOND)) as u16,
            n if n <= 10 * MILLISECOND => 100 + ((n - MILLISECOND) / (100 * MICROSECOND)) as u16,
            n if n <= 100 * MILLISECOND => 190 + ((n - 10 * MILLISECOND) / MILLISECOND) as u16,
            n if n <= SECOND => 280 + ((n - 100 * MILLISECOND) / (10 * MILLISECOND)) as u16,
            n if n <= 10 * SECOND => 370 + ((n - SECOND) / (100 * MILLISECOND)) as u16,
            _ => 460,
        }
    }

    /// Converts a 16-bit symbol back to a duration in nanoseconds.
    pub fn symbol_to_duration_ns(&self, symbol: u16) -> u64 {
        const MICROSECOND: u64 = 1_000;
        const MILLISECOND: u64 = 1_000_000;
        const SECOND: u64 = 1_000_000_000;

        match symbol {
            s @ 0..=100 => s as u64 * 10 * MICROSECOND,
            s @ 101..=190 => MILLISECOND + (s - 100) as u64 * 100 * MICROSECOND,
            s @ 191..=280 => 10 * MILLISECOND + (s - 190) as u64 * MILLISECOND,
            s @ 281..=370 => 100 * MILLISECOND + (s - 280) as u64 * 10 * MILLISECOND,
            s @ 371..=460 => SECOND + (s - 370) as u64 * 100 * MILLISECOND,
            _ => 10 * SECOND,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quantizer_status_to_symbol() {
        let q = Quantizer::new();
        assert_eq!(q.status_to_symbol(&PingStatus::Timeout), 65530);
        assert_eq!(q.status_to_symbol(&PingStatus::IOError), 65531);
        assert_eq!(q.status_to_symbol(&PingStatus::Partial), 65532);
        assert_eq!(q.status_to_symbol(&PingStatus::Skipped), 65533);
        assert_eq!(q.status_to_symbol(&PingStatus::Other), 65534);
    }

    #[test]
    fn test_quantizer_symbol_to_status_roundtrip() {
        let q = Quantizer::new();
        let statuses = vec![
            PingStatus::Success(50_000),
            PingStatus::Success(1_500_000),
            PingStatus::Success(50_000_000),
            PingStatus::Timeout,
            PingStatus::IOError,
            PingStatus::Partial,
            PingStatus::Skipped,
            PingStatus::Other,
        ];

        for status in statuses {
            let symbol = q.status_to_symbol(&status);
            let recovered_status = q.symbol_to_status(symbol);

            match (status, recovered_status) {
                (PingStatus::Success(original_ns), PingStatus::Success(recovered_ns)) => {
                    let diff = (original_ns as i64 - recovered_ns as i64).abs();
                    assert!(diff < 100_000, "Quantization error too high: {}", diff);
                }
                (a, b) => assert_eq!(a, b),
            }
        }
    }

    #[test]
    fn test_quantizer_duration_to_symbol() {
        let q = Quantizer::new();
        assert_eq!(q.duration_ns_to_symbol(0), 0);
        assert_eq!(q.duration_ns_to_symbol(1_000_000), 100);
        assert_eq!(q.duration_ns_to_symbol(10_000_000), 190);
        assert_eq!(q.duration_ns_to_symbol(100_000_000), 280);
        assert_eq!(q.duration_ns_to_symbol(1_000_000_000), 370);
        assert_eq!(q.duration_ns_to_symbol(10_000_000_000), 460);
    }

    #[test]
    fn test_compress_batch_empty() {
        let batch = PingBatch::new();
        let compressed = compress_batch(&batch).unwrap();
        assert!(compressed.is_empty());
    }

    #[test]
    fn test_compress_batch_groups_by_target() {
        let batch = vec![
            PingResult {
                target: "8.8.8.8".to_string(),
                sent_time_ns: 100,
                status: PingStatus::Success(50),
            },
            PingResult {
                target: "1.1.1.1".to_string(),
                sent_time_ns: 110,
                status: PingStatus::Success(60),
            },
        ];

        let compressed = compress_batch(&batch).unwrap();
        let s = String::from_utf8_lossy(&compressed);

        assert!(s.contains("8.8.8.8"));
        assert!(s.contains("1.1.1.1"));
    }

    #[test]
    fn test_compression_decompression_roundtrip() {
        let batch = vec![
            PingResult {
                target: "8.8.8.8".to_string(),
                sent_time_ns: 1_000_000_000,
                status: PingStatus::Success(50_000),
            },
            PingResult {
                target: "8.8.8.8".to_string(),
                sent_time_ns: 1_033_000_000, // 33ms interval
                status: PingStatus::Success(52_000),
            },
            PingResult {
                target: "8.8.8.8".to_string(),
                sent_time_ns: 1_066_000_000, // 33ms interval
                status: PingStatus::Timeout,
            },
        ];

        let compressed = compress_batch(&batch).unwrap();
        let decompressed = decompress_batch(&compressed).unwrap();

        assert_eq!(batch.len(), decompressed.len());
        for (original, recovered) in batch.iter().zip(decompressed.iter()) {
            assert_eq!(original.target, recovered.target);
            // Timestamps for variable rate are quantized, so we need to allow a small error margin.
            let time_diff = (original.sent_time_ns as i64 - recovered.sent_time_ns as i64).abs();
            assert!(time_diff < 100_000, "Timestamp quantization error too high: {}", time_diff);

            match (&original.status, &recovered.status) {
                (PingStatus::Success(original_ns), PingStatus::Success(recovered_ns)) => {
                    let diff = (*original_ns as i64 - *recovered_ns as i64).abs();
                    assert!(diff < 100_000, "Quantization error too high: {}", diff);
                }
                (a, b) => assert_eq!(a, b),
            }
        }
    }
}
