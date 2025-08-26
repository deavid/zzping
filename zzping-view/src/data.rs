use anyhow::{Result, anyhow, bail};
use byteorder::{LittleEndian, ReadBytesExt};
use chrono::{DateTime, Duration, TimeZone, Utc};
use std::io::Cursor;

// Magic number for zzping-capture format (0x7A7A504E47434150 = "zzPNGCAP")
const CAPTURE_MAGIC: u64 = 0x7A7A504E47434150;
const HEADER_SIZE: usize = 16;
// zzping-capture format uses 16 bytes per record (u64 sent_nanos + u64 rtt_nanos)
const RECORD_SIZE: usize = 16;

/// A single ping data point with proper type safety for RTT values.
#[derive(Debug, Clone)]
pub struct DataPoint {
    /// The timestamp when this ping was sent
    pub time: DateTime<Utc>,
    /// Round-trip time, or None if the packet was lost
    pub rtt: Option<Duration>,
}

/// The entire set of loaded ping data.
pub struct PingData {
    pub points: Vec<DataPoint>,
}

/// Loads and parses ping data with an optional limit on the number of records.
pub fn load_and_parse_with_limit(raw_data: &[u8], limit: Option<usize>) -> Result<PingData> {
    if raw_data.len() < HEADER_SIZE {
        bail!(
            "Invalid data: file size ({} bytes) is smaller than header size ({} bytes).",
            raw_data.len(),
            HEADER_SIZE
        );
    }

    let mut cursor = Cursor::new(raw_data);

    // 1. Read and validate header
    let magic = cursor.read_u64::<LittleEndian>()?;
    if magic != CAPTURE_MAGIC {
        bail!(
            "Invalid file format. Expected zzping-capture magic {:016X}, got {:016X}.\nMake sure you're opening a .dat file from zzping-capture, not a compressed .dqv1 file.",
            CAPTURE_MAGIC,
            magic
        );
    }

    let start_timestamp_ns = cursor.read_u64::<LittleEndian>()?;
    let start_time = Utc
        .timestamp_opt(
            (start_timestamp_ns / 1_000_000_000) as i64,
            (start_timestamp_ns % 1_000_000_000) as u32,
        )
        .single()
        .ok_or_else(|| anyhow!("Invalid start timestamp {} in header", start_timestamp_ns))?;

    // 2. Validate data size
    let remaining_bytes = raw_data.len() - HEADER_SIZE;
    if remaining_bytes % RECORD_SIZE != 0 {
        bail!(
            "Invalid data: remaining {} bytes is not divisible by record size {} bytes",
            remaining_bytes,
            RECORD_SIZE
        );
    }

    let num_records = remaining_bytes / RECORD_SIZE;
    let actual_records = if let Some(limit) = limit {
        num_records.min(limit)
    } else {
        num_records
    };
    let mut points = Vec::with_capacity(actual_records);

    if let Some(_limit) = limit {
        eprintln!(
            "Loading {} ping records from zzping-capture format (limited from {} total)",
            actual_records, num_records
        );
    } else {
        eprintln!(
            "Loading {} ping records from zzping-capture format",
            num_records
        );
    }
    eprintln!(
        "Capture started at: {}",
        start_time.format("%Y-%m-%d %H:%M:%S UTC")
    );

    let mut lost_count = 0;

    for record_idx in 0..actual_records {
        // Each record contains:
        // - sent_nanos: u64 (nanoseconds since start_time when ping was sent)
        // - rtt_nanos: u64 (RTT in nanoseconds, or u64::MAX for lost packets)
        let sent_nanos = cursor.read_u64::<LittleEndian>()?;
        let rtt_nanos = cursor.read_u64::<LittleEndian>()?;

        // Convert sent_nanos to absolute timestamp
        let sent_time = start_time + Duration::nanoseconds(sent_nanos as i64);

        // Convert RTT to Duration, handling packet loss
        let rtt = if rtt_nanos == u64::MAX {
            lost_count += 1;
            None // Packet was lost
        } else {
            Some(Duration::nanoseconds(rtt_nanos as i64))
        };

        points.push(DataPoint {
            time: sent_time,
            rtt,
        });

        // Log progress for large files
        if record_idx > 0 && record_idx % 100_000 == 0 {
            eprintln!("Processed {} records...", record_idx);
        }
    }

    let success_rate = ((actual_records - lost_count) as f64 / actual_records as f64) * 100.0;
    eprintln!("Successfully loaded {} ping data points", points.len());
    eprintln!(
        "Packet loss: {} / {} ({:.2}%)",
        lost_count,
        actual_records,
        100.0 - success_rate
    );

    if points.is_empty() {
        bail!("No valid data points found in file");
    }

    Ok(PingData { points })
}
