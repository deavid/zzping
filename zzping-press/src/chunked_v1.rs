use crate::RawDataRecord;
use anyhow::{anyhow, Result};
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use constriction::stream::{
    model::{
        DefaultNonContiguousCategoricalDecoderModel, DefaultNonContiguousCategoricalEncoderModel,
    },
    stack::DefaultAnsCoder,
    Decode, Encode,
};
use std::io::{Cursor, Read, Write};
use std::time::Duration;

// --- Quantization ---

const PACKET_LOST_SYMBOL: u16 = 65535;

pub struct Quantizer {
    ln_1_001: f64,
}

impl Quantizer {
    pub fn new() -> Self {
        Self {
            ln_1_001: 1.001f64.ln(),
        }
    }

    pub fn duration_to_symbol(&self, d: Duration) -> u16 {
        let time_in_ms = d.as_secs_f64() * 1000.0;
        if time_in_ms <= 0.0 {
            return 0;
        }
        let encoded_value = (time_in_ms / 100.0 + 1.0).ln() / self.ln_1_001;
        let symbol = encoded_value.round() as u16;
        if symbol == PACKET_LOST_SYMBOL {
            PACKET_LOST_SYMBOL - 1
        } else {
            symbol
        }
    }

    pub fn symbol_to_duration(&self, symbol: u16) -> Duration {
        if symbol == PACKET_LOST_SYMBOL {
            return Duration::from_secs(u64::MAX);
        }
        let time_in_ms = (self.ln_1_001 * symbol as f64).exp() - 1.0;
        let time_in_ms = time_in_ms * 100.0;
        Duration::from_secs_f64(time_in_ms / 1000.0)
    }
}

impl Default for Quantizer {
    fn default() -> Self {
        Self::new()
    }
}

// --- File Format Structs ---

pub const FILE_MAGIC: u64 = 0x5A5A504356312020;
pub const FORMAT_VERSION: u16 = 1;
pub const HEADER_SIZE: usize = 65536;

pub struct FileHeader {
    pub magic: u64,
    pub format_version: u16,
    pub start_time_unix_ns: u64,
    pub aggregate_entry_count: u32,
    pub index_entry_count: u32,
}

impl FileHeader {
    pub fn write(&self, mut w: impl Write) -> std::io::Result<()> {
        w.write_u64::<BigEndian>(self.magic)?;
        w.write_u16::<BigEndian>(self.format_version)?;
        w.write_u64::<BigEndian>(self.start_time_unix_ns)?;
        w.write_u32::<BigEndian>(self.aggregate_entry_count)?;
        w.write_u32::<BigEndian>(self.index_entry_count)?;
        Ok(())
    }

    pub fn read(mut r: impl Read) -> std::io::Result<Self> {
        let magic = r.read_u64::<BigEndian>()?;
        let format_version = r.read_u16::<BigEndian>()?;
        let start_time_unix_ns = r.read_u64::<BigEndian>()?;
        let aggregate_entry_count = r.read_u32::<BigEndian>()?;
        let index_entry_count = r.read_u32::<BigEndian>()?;
        Ok(Self {
            magic,
            format_version,
            start_time_unix_ns,
            aggregate_entry_count,
            index_entry_count,
        })
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AggregateEntry {
    pub p00_symbol: u16,
    pub p10_symbol: u16,
    pub p20_symbol: u16,
    pub p30_symbol: u16,
    pub p40_symbol: u16,
    pub p50_symbol: u16,
    pub p60_symbol: u16,
    pub p70_symbol: u16,
    pub p80_symbol: u16,
    pub p90_symbol: u16,
    pub p100_symbol: u16,
    pub lost_packet_count: u32, // Only relevant for RTT stats.
}

impl AggregateEntry {
    pub fn write(&self, mut w: impl Write) -> std::io::Result<()> {
        w.write_u16::<BigEndian>(self.p00_symbol)?;
        w.write_u16::<BigEndian>(self.p10_symbol)?;
        w.write_u16::<BigEndian>(self.p20_symbol)?;
        w.write_u16::<BigEndian>(self.p30_symbol)?;
        w.write_u16::<BigEndian>(self.p40_symbol)?;
        w.write_u16::<BigEndian>(self.p50_symbol)?;
        w.write_u16::<BigEndian>(self.p60_symbol)?;
        w.write_u16::<BigEndian>(self.p70_symbol)?;
        w.write_u16::<BigEndian>(self.p80_symbol)?;
        w.write_u16::<BigEndian>(self.p90_symbol)?;
        w.write_u16::<BigEndian>(self.p100_symbol)?;
        w.write_u32::<BigEndian>(self.lost_packet_count)?;
        Ok(())
    }

    pub fn read(mut r: impl Read) -> std::io::Result<Self> {
        Ok(Self {
            p00_symbol: r.read_u16::<BigEndian>()?,
            p10_symbol: r.read_u16::<BigEndian>()?,
            p20_symbol: r.read_u16::<BigEndian>()?,
            p30_symbol: r.read_u16::<BigEndian>()?,
            p40_symbol: r.read_u16::<BigEndian>()?,
            p50_symbol: r.read_u16::<BigEndian>()?,
            p60_symbol: r.read_u16::<BigEndian>()?,
            p70_symbol: r.read_u16::<BigEndian>()?,
            p80_symbol: r.read_u16::<BigEndian>()?,
            p90_symbol: r.read_u16::<BigEndian>()?,
            p100_symbol: r.read_u16::<BigEndian>()?,
            lost_packet_count: r.read_u32::<BigEndian>()?,
        })
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct IndexEntry {
    pub chunk_offset_bytes: u64,
}

impl IndexEntry {
    pub fn write(&self, mut w: impl Write) -> std::io::Result<()> {
        w.write_u64::<BigEndian>(self.chunk_offset_bytes)?;
        Ok(())
    }

    pub fn read(mut r: impl Read) -> std::io::Result<Self> {
        Ok(Self {
            chunk_offset_bytes: r.read_u64::<BigEndian>()?,
        })
    }
}

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct ChunkFlags: u8 {
        const IS_VARIABLE_RATE = 0b00000001;
        const RAW_DELTAS = 0b00000010;
    }
}

pub struct ChunkHeader {
    pub start_time_unix_ns: u64,
    pub rtt_symbol_count: u32,
    pub send_time_symbol_count: u32,
    pub rtt_stream_len_bytes: u32,
    pub send_time_stream_len_bytes: u32,
    pub flags: ChunkFlags,
    pub constant_rate_avg_interval: f64,
    pub rtt_stats: AggregateEntry,
    pub send_time_stats: Option<AggregateEntry>,
}

impl ChunkHeader {
    pub fn write(&self, mut w: impl Write) -> std::io::Result<()> {
        w.write_u64::<BigEndian>(self.start_time_unix_ns)?;
        w.write_u32::<BigEndian>(self.rtt_symbol_count)?;
        w.write_u32::<BigEndian>(self.send_time_symbol_count)?;
        w.write_u32::<BigEndian>(self.rtt_stream_len_bytes)?;
        w.write_u32::<BigEndian>(self.send_time_stream_len_bytes)?;
        w.write_u8(self.flags.bits())?;
        w.write_f64::<BigEndian>(self.constant_rate_avg_interval)?;
        self.rtt_stats.write(&mut w)?;
        if let Some(stats) = &self.send_time_stats {
            stats.write(&mut w)?;
        }
        Ok(())
    }

    pub fn read(mut r: impl Read) -> Result<Self, std::io::Error> {
        let start_time_unix_ns = r.read_u64::<BigEndian>()?;
        let rtt_symbol_count = r.read_u32::<BigEndian>()?;
        let send_time_symbol_count = r.read_u32::<BigEndian>()?;
        let rtt_stream_len_bytes = r.read_u32::<BigEndian>()?;
        let send_time_stream_len_bytes = r.read_u32::<BigEndian>()?;
        let flags = ChunkFlags::from_bits_truncate(r.read_u8()?);
        let constant_rate_avg_interval = r.read_f64::<BigEndian>()?;
        let rtt_stats = AggregateEntry::read(&mut r)?;

        let send_time_stats = if flags.contains(ChunkFlags::IS_VARIABLE_RATE) && !flags.contains(ChunkFlags::RAW_DELTAS) {
            Some(AggregateEntry::read(&mut r)?)
        } else {
            None
        };

        Ok(Self {
            start_time_unix_ns,
            rtt_symbol_count,
            send_time_symbol_count,
            rtt_stream_len_bytes,
            send_time_stream_len_bytes,
            flags,
            constant_rate_avg_interval,
            rtt_stats,
            send_time_stats,
        })
    }
}

// --- Compression Logic ---

fn calculate_duration_stats(
    durations: &[Duration],
    quantizer: &Quantizer,
) -> AggregateEntry {
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

enum SendTimeStrategy {
    ConstantRate { avg_interval: f64 },
    VariableRate { deltas: Vec<Duration> },
}

fn analyze_send_times(records: &[RawDataRecord]) -> SendTimeStrategy {
    if records.len() < 2 {
        return SendTimeStrategy::ConstantRate { avg_interval: 0.0 };
    }

    let total_duration = records.last().unwrap().sent_nanos - records.first().unwrap().sent_nanos;
    let avg_interval = total_duration as f64 / (records.len() - 1) as f64;

    if avg_interval == 0.0 {
        let deltas = records
            .windows(2)
            .map(|r| Duration::from_nanos(r[1].sent_nanos - r[0].sent_nanos))
            .collect();
        return SendTimeStrategy::VariableRate { deltas };
    }

    let first_ts = records.first().unwrap().sent_nanos;
    let mut max_drift_ns: f64 = 0.0;

    for (i, record) in records.iter().enumerate() {
        let ideal_ts = first_ts as f64 + (i as f64 * avg_interval);
        let drift = (record.sent_nanos as f64 - ideal_ts).abs();
        max_drift_ns = max_drift_ns.max(drift);
    }

    const DRIFT_THRESHOLD_NS: f64 = 2_000_000.0; // 2ms

    if max_drift_ns <= DRIFT_THRESHOLD_NS {
        SendTimeStrategy::ConstantRate { avg_interval }
    } else {
        let deltas = records
            .windows(2)
            .map(|r| Duration::from_nanos(r[1].sent_nanos - r[0].sent_nanos))
            .collect();
        SendTimeStrategy::VariableRate { deltas }
    }
}

fn build_model(
    stats: &AggregateEntry,
    symbol_count: usize,
) -> Result<(Vec<u16>, Vec<f64>)> {
    let mut frequencies = vec![0u32; u16::MAX as usize + 1];
    let valid_symbols_count = symbol_count - stats.lost_packet_count as usize;

    if valid_symbols_count > 0 {
        let percentile_points = [
            stats.p00_symbol, stats.p10_symbol, stats.p20_symbol, stats.p30_symbol,
            stats.p40_symbol, stats.p50_symbol, stats.p60_symbol, stats.p70_symbol,
            stats.p80_symbol, stats.p90_symbol, stats.p100_symbol,
        ];

        if valid_symbols_count < 10 {
            for &p in &percentile_points {
                frequencies[p as usize] += 1;
            }
        } else {
            let bucket_count = valid_symbols_count / 10;
            for i in 0..10 {
                let start_symbol = percentile_points[i] as usize;
                let end_symbol = percentile_points[i + 1] as usize;
                if start_symbol > end_symbol { continue; }
                let symbol_range_size = (end_symbol - start_symbol) + 1;
                let freq = (bucket_count as f64 / symbol_range_size as f64).ceil() as u32;
                let freq = freq.max(1);
                for s in start_symbol..=end_symbol {
                    frequencies[s] = freq;
                }
            }
        }
    }

    if stats.lost_packet_count > 0 {
        let total_valid_freq: u32 = frequencies.iter().sum();
        if valid_symbols_count > 0 {
            let avg_freq_per_symbol = total_valid_freq as f64 / valid_symbols_count as f64;
            let lost_freq = (avg_freq_per_symbol * stats.lost_packet_count as f64).round() as u32;
            frequencies[PACKET_LOST_SYMBOL as usize] = lost_freq.max(1);
        } else {
            frequencies[PACKET_LOST_SYMBOL as usize] = 1;
        }
    }

    let (mut symbols_with_freq, mut probabilities): (Vec<_>, Vec<_>) = frequencies
        .iter()
        .enumerate()
        .filter(|&(_, f)| *f > 0)
        .map(|(s, f)| (s as u16, *f))
        .unzip();

    if symbols_with_freq.len() == 1 {
        let dummy_symbol = if symbols_with_freq.is_empty() || symbols_with_freq[0] == 0 { 1 } else { 0 };
        symbols_with_freq.push(dummy_symbol);
        probabilities.push(1);
    }

    if symbols_with_freq.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }

    let total_freq: u32 = probabilities.iter().sum();
    if total_freq == 0 {
        return Ok((Vec::new(), Vec::new()));
    }

    let mut probabilities_f64: Vec<f64> = probabilities
        .iter()
        .map(|&f| f as f64 / total_freq as f64)
        .collect();

    let total_prob = probabilities_f64.iter().sum::<f64>();
    if let Some(last_prob) = probabilities_f64.last_mut() {
        *last_prob += 1.0 - total_prob;
    }

    Ok((symbols_with_freq, probabilities_f64))
}

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

    let mut payload_buffer = Vec::new();
    let mut aggregate_entries = Vec::new();
    let mut index_entries = Vec::new();

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
        let mut constant_rate_avg_interval = 0.0;

        let (send_time_encoded_data, send_time_symbol_count, send_time_stats) =
            match &send_time_strategy {
                SendTimeStrategy::ConstantRate { avg_interval } => {
                    constant_rate_avg_interval = *avg_interval;
                    (Vec::new(), 0, None)
                }
                SendTimeStrategy::VariableRate { deltas } => {
                    flags |= ChunkFlags::IS_VARIABLE_RATE;
                    flags |= ChunkFlags::RAW_DELTAS;
                    let mut data = Vec::new();
                    for delta in deltas {
                        data.write_u64::<BigEndian>(delta.as_nanos() as u64)?;
                    }
                    (data, deltas.len() as u32, None)
                }
            };

        let (rtt_s, rtt_p) = build_model(&rtt_stats, rtt_symbols.len())?;
        let rtt_encoded_data = if !rtt_s.is_empty() {
            let rtt_model = DefaultNonContiguousCategoricalEncoderModel::from_symbols_and_floating_point_probabilities_fast(rtt_s, &rtt_p, None)
                .map_err(|_| anyhow!("Failed to create categorical model"))?;
            let mut rtt_encoder = DefaultAnsCoder::new();
            rtt_encoder.encode_iid_symbols_reverse(&rtt_symbols, &rtt_model)?;
            let rtt_encoded_data_u32 = rtt_encoder.into_compressed()?;
            rtt_encoded_data_u32.iter().flat_map(|w| w.to_be_bytes()).collect()
        } else {
            Vec::new()
        };

        let chunk_header = ChunkHeader {
            start_time_unix_ns: chunk_records.first().map_or(0, |r| r.sent_nanos),
            rtt_symbol_count: rtt_symbols.len() as u32,
            send_time_symbol_count,
            rtt_stream_len_bytes: rtt_encoded_data.len() as u32,
            send_time_stream_len_bytes: send_time_encoded_data.len() as u32,
            flags,
            constant_rate_avg_interval,
            rtt_stats,
            send_time_stats,
        };

        let mut chunk_buffer = Vec::new();
        chunk_header.write(&mut chunk_buffer)?;
        chunk_buffer.extend_from_slice(&rtt_encoded_data);
        chunk_buffer.extend_from_slice(&send_time_encoded_data);

        index_entries.push(IndexEntry {
            chunk_offset_bytes: (HEADER_SIZE + payload_buffer.len()) as u64,
        });
        payload_buffer.extend_from_slice(&chunk_buffer);
    }

    // --- 3. Finalize the file ---
    let mut header_buf = vec![0u8; HEADER_SIZE];

    let file_header = FileHeader {
        magic: FILE_MAGIC,
        format_version: FORMAT_VERSION,
        start_time_unix_ns: records.first().map_or(0, |r| r.sent_nanos),
        aggregate_entry_count: aggregate_entries.len() as u32,
        index_entry_count: index_entries.len() as u32,
    };

    let mut cursor = Cursor::new(&mut header_buf[..]);
    file_header.write(&mut cursor)?;

    for entry in &aggregate_entries {
        entry.write(&mut cursor)?;
    }

    for entry in &index_entries {
        entry.write(&mut cursor)?;
    }

    let mut final_data = header_buf;
    final_data.extend_from_slice(&payload_buffer);

    Ok(final_data)
}

fn build_model_for_decode(
    stats: &AggregateEntry,
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

    let mut aggregate_table = Vec::with_capacity(file_header.aggregate_entry_count as usize);
    for _ in 0..file_header.aggregate_entry_count {
        aggregate_table.push(AggregateEntry::read(&mut cursor)?);
    }

    let mut index_table = Vec::with_capacity(file_header.index_entry_count as usize);
    for _ in 0..file_header.index_entry_count {
        index_table.push(IndexEntry::read(&mut cursor)?);
    }

    let quantizer = Quantizer::new();
    let mut all_records = Vec::new();

    for index_entry in &index_table {
        let chunk_offset = index_entry.chunk_offset_bytes as usize;
        let mut chunk_cursor = Cursor::new(&data[chunk_offset..]);
        let chunk_header = ChunkHeader::read(&mut chunk_cursor)?;

        let header_len = chunk_cursor.position() as usize;

        let rtt_stream_start = chunk_offset + header_len;
        let rtt_stream_end = rtt_stream_start + chunk_header.rtt_stream_len_bytes as usize;
        let rtt_data_u8 = &data[rtt_stream_start .. rtt_stream_end];

        let rtt_symbols = if chunk_header.rtt_stream_len_bytes > 0 {
            let rtt_data_u32: Vec<u32> = rtt_data_u8
                .chunks_exact(4)
                .map(|b| u32::from_be_bytes(b.try_into().unwrap()))
                .collect();

            let rtt_model = build_model_for_decode(&chunk_header.rtt_stats, chunk_header.rtt_symbol_count as usize)?;
            let mut decoder = DefaultAnsCoder::from_compressed(rtt_data_u32)
                .map_err(|_| anyhow!("Invalid compressed data for RTT stream"))?;
            decoder.decode_iid_symbols(chunk_header.rtt_symbol_count as usize, &rtt_model).collect::<Result<Vec<_>,_>>()?
        } else {
            vec![chunk_header.rtt_stats.p00_symbol; chunk_header.rtt_symbol_count as usize]
        };

        let mut current_sent_nanos = chunk_header.start_time_unix_ns as f64;
        for i in 0..chunk_header.rtt_symbol_count as usize {
            let rtt_nanos = if rtt_symbols[i] == PACKET_LOST_SYMBOL {
                u64::MAX
            } else {
                quantizer.symbol_to_duration(rtt_symbols[i]).as_nanos() as u64
            };

            if i > 0 {
                if chunk_header.flags.contains(ChunkFlags::RAW_DELTAS) {
                    let send_time_stream_start = rtt_stream_end;
                    let delta_offset = send_time_stream_start + (i - 1) * 8;
                    let mut delta_cursor = Cursor::new(&data[delta_offset..]);
                    let delta = delta_cursor.read_u64::<BigEndian>()?;
                    current_sent_nanos += delta as f64;
                } else {
                    current_sent_nanos = chunk_header.start_time_unix_ns as f64 + (i as f64 * chunk_header.constant_rate_avg_interval);
                }
            }

            all_records.push(RawDataRecord {
                sent_nanos: current_sent_nanos.round() as u64,
                rtt_nanos,
            });
        }
    }

    Ok(all_records)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_model_creation() {
        let stats = AggregateEntry {
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
        let (s, p) = build_model(&stats, 100).unwrap();
        let model = DefaultNonContiguousCategoricalEncoderModel::from_symbols_and_floating_point_probabilities_fast(s, &p, None);
        assert!(model.is_ok());
    }

    #[test]
    fn test_single_symbol_model_creation() {
        let stats = AggregateEntry {
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
        let result = build_model(&stats, 1);
        assert!(result.is_ok(), "Failed with error: {:?}", result.err());
    }
}
