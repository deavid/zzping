use anyhow::{anyhow, bail, Result};
use byteorder::{LittleEndian, ReadBytesExt};
use chrono::{DateTime, Duration, TimeZone, Utc};
use std::io::Cursor;

const MAGIC_NUMBER: u64 = 0x7A7A505245535331;
const HEADER_SIZE: usize = 16;
const RECORD_SIZE: usize = 4;

/// A single, reconstituted ping data point.
#[derive(Debug, Clone)]
pub struct DataPoint {
    pub time: DateTime<Utc>,
    /// Round-trip time in microseconds. u16::MAX means a lost packet.
    pub rtt_micros: u16,
}

/// The entire set of loaded ping data.
pub struct PingData {
    pub points: Vec<DataPoint>,
}

/// Loads and parses the compressed ping data from a byte buffer.
pub fn load_and_parse(raw_data: &[u8]) -> Result<PingData> {
    if raw_data.len() < HEADER_SIZE {
        bail!(
            "Invalid data: file size ({}) is smaller than header size ({}).",
            raw_data.len(),
            HEADER_SIZE
        );
    }

    let mut cursor = Cursor::new(raw_data);

    // 1. Read Header
    let magic = cursor.read_u64::<LittleEndian>()?;
    if magic != MAGIC_NUMBER {
        bail!(
            "Invalid magic number. Expected {:X}, got {:X}.",
            MAGIC_NUMBER,
            magic
        );
    }

    let start_ts_nanos = cursor.read_u64::<LittleEndian>()?;
    let start_time = Utc
        .timestamp_opt(
            (start_ts_nanos / 1_000_000_000) as i64,
            (start_ts_nanos % 1_000_000_000) as u32,
        )
        .single()
        .ok_or_else(|| anyhow!("Invalid start timestamp in header"))?;

    // 2. Read Data Records
    let mut current_time = start_time;
    let num_records = (raw_data.len() - HEADER_SIZE) / RECORD_SIZE;
    let mut points = Vec::with_capacity(num_records);

    for _ in 0..num_records {
        let sent_delta_micros = cursor.read_u16::<LittleEndian>()?;
        let rtt_micros = cursor.read_u16::<LittleEndian>()?;

        current_time += Duration::microseconds(sent_delta_micros as i64);

        points.push(DataPoint {
            time: current_time,
            rtt_micros,
        });
    }

    Ok(PingData { points })
}
