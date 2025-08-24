use anyhow::{bail, Context, Result};
use clap::Parser;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::marker::PhantomData;
use std::path::PathBuf;

const CAPTURE_MAGIC: u64 = 0x7A7A504E47434150; // zzPNGCAP
const PRESS_MAGIC_V1: u64 = 0x7A7A505245535331; // zzPRESS1

// --- Data Structs ---

#[derive(Debug, Clone, Copy)]
struct CaptureHeader {
    start_timestamp_ns: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RawDataRecord {
    sent_nanos: u64,
    rtt_nanos: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CompressedDataRecord {
    sent_delta_micros: u16,
    rtt_micros: u16,
}

// --- Generic Record Reading Infrastructure ---

trait FromBytes: Sized {
    const SIZE: usize;
    fn from_le_bytes(bytes: &[u8]) -> Result<Self>;
}

impl FromBytes for RawDataRecord {
    const SIZE: usize = 16;
    fn from_le_bytes(bytes: &[u8]) -> Result<Self> {
        let sent_nanos = u64::from_le_bytes(bytes[0..8].try_into()?);
        let rtt_nanos = u64::from_le_bytes(bytes[8..16].try_into()?);
        Ok(Self { sent_nanos, rtt_nanos })
    }
}

impl FromBytes for CompressedDataRecord {
    const SIZE: usize = 4;
    fn from_le_bytes(bytes: &[u8]) -> Result<Self> {
        let sent_delta_micros = u16::from_le_bytes(bytes[0..2].try_into()?);
        let rtt_micros = u16::from_le_bytes(bytes[2..4].try_into()?);
        Ok(Self { sent_delta_micros, rtt_micros })
    }
}

struct RecordIterator<R: Read, T: FromBytes> {
    reader: R,
    _phantom: PhantomData<T>,
}

impl<R: Read, T: FromBytes> RecordIterator<R, T> {
    fn new(reader: R) -> Self {
        Self { reader, _phantom: PhantomData }
    }
}

impl<R: Read, T: FromBytes> Iterator for RecordIterator<R, T> {
    type Item = Result<T>;
    fn next(&mut self) -> Option<Self::Item> {
        let mut buffer = vec![0; T::SIZE];
        match self.reader.read_exact(&mut buffer) {
            Ok(()) => Some(T::from_le_bytes(&buffer)),
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => None,
            Err(e) => Some(Err(e.into())),
        }
    }
}

// --- CLI Definition ---

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[arg(long, value_name = "FILE_PATH")]
    input: PathBuf,
    #[arg(long, value_name = "FILE_PATH")]
    output: PathBuf,
    #[arg(long, value_name = "STRATEGY_NAME")]
    strategy: Strategy,
}

#[derive(clap::ValueEnum, Clone, Debug)]
enum Strategy {
    DeltaQuantizedV1,
}

// --- Main Application Logic ---

fn main() -> Result<()> {
    let cli = Cli::parse();

    // --- Compression Phase ---
    let (header, raw_records_iterator) = read_raw_records(&cli.input)?;
    let mut records: Vec<RawDataRecord> = raw_records_iterator.collect::<Result<_>>()?;

    let mut writer = BufWriter::new(File::create(&cli.output)
        .with_context(|| format!("Failed to create output file: {}", cli.output.display()))?);
    write_press_header(&mut writer, header.start_timestamp_ns)?;

    let records_written = match cli.strategy {
        Strategy::DeltaQuantizedV1 => {
            stream_compress_delta_quantized_v1(&mut records, &mut writer)?
        }
    };
    println!(
        "Successfully wrote {} compressed records to {}.",
        records_written,
        cli.output.display()
    );

    // --- Verification Phase ---
    let compressed_records_iterator = read_compressed_records(&cli.output)?;
    let read_compressed_records: Vec<_> = compressed_records_iterator.collect::<Result<_>>()?;
    let decompressed_records = decompress_delta_quantized_v1(&read_compressed_records);

    print_metrics(&cli.input, &read_compressed_records, &decompressed_records)?;

    Ok(())
}

// --- File I/O Functions ---

fn read_raw_records(
    path: &PathBuf,
) -> Result<(CaptureHeader, RecordIterator<BufReader<File>, RawDataRecord>)> {
    let file = File::open(path)
        .with_context(|| format!("Failed to open input file: {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut header_buf = [0u8; 16];
    reader.read_exact(&mut header_buf).context("Failed to read capture header")?;
    let magic = u64::from_le_bytes(header_buf[0..8].try_into()?);
    if magic != CAPTURE_MAGIC {
        bail!("Invalid magic number in input file. Expected {CAPTURE_MAGIC:x}, found {magic:x}");
    }
    let start_timestamp_ns = u64::from_le_bytes(header_buf[8..16].try_into()?);
    let header = CaptureHeader { start_timestamp_ns };
    Ok((header, RecordIterator::new(reader)))
}

fn read_compressed_records(
    path: &PathBuf,
) -> Result<RecordIterator<BufReader<File>, CompressedDataRecord>> {
    let file = File::open(path)
        .with_context(|| format!("Failed to open compressed file: {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut header_buf = [0u8; 16];
    reader.read_exact(&mut header_buf).context("Failed to read press header")?;
    let magic = u64::from_le_bytes(header_buf[0..8].try_into()?);
    if magic != PRESS_MAGIC_V1 {
        bail!("Invalid magic number in compressed file. Expected {PRESS_MAGIC_V1:x}, found {magic:x}");
    }
    // Read the timestamp to advance the reader, but we don't use it.
    let _original_start_timestamp_ns = u64::from_le_bytes(header_buf[8..16].try_into()?);
    Ok(RecordIterator::new(reader))
}

fn write_press_header(writer: &mut impl Write, original_start_timestamp_ns: u64) -> Result<()> {
    writer.write_all(&PRESS_MAGIC_V1.to_le_bytes())?;
    writer.write_all(&original_start_timestamp_ns.to_le_bytes())?;
    Ok(())
}

// --- Core Logic ---

fn stream_compress_delta_quantized_v1(
    records: &mut [RawDataRecord],
    writer: &mut impl Write,
) -> Result<usize> {
    records.sort_by_key(|r| r.sent_nanos);
    let mut last_sent_nanos: u64 = 0;
    for record in records.iter() {
        let sent_delta_nanos = record.sent_nanos - last_sent_nanos;
        let sent_delta_us = sent_delta_nanos / 1000;
        let sent_delta_micros = (sent_delta_us).min(u16::MAX as u64) as u16;
        last_sent_nanos = record.sent_nanos;
        let rtt_micros = if record.rtt_nanos == u64::MAX {
            u16::MAX
        } else {
            let rtt_us = record.rtt_nanos / 1000;
            (rtt_us).min((u16::MAX - 1) as u64) as u16
        };
        writer.write_all(&sent_delta_micros.to_le_bytes())?;
        writer.write_all(&rtt_micros.to_le_bytes())?;
    }
    Ok(records.len())
}

fn decompress_delta_quantized_v1(compressed_records: &[CompressedDataRecord]) -> Vec<RawDataRecord> {
    let mut decompressed_records = Vec::with_capacity(compressed_records.len());
    let mut last_sent_nanos: u64 = 0;
    for compressed in compressed_records {
        let sent_nanos = last_sent_nanos.saturating_add(compressed.sent_delta_micros as u64 * 1000);
        last_sent_nanos = sent_nanos;
        let rtt_nanos = if compressed.rtt_micros == u16::MAX {
            u64::MAX
        } else {
            compressed.rtt_micros as u64 * 1000
        };
        decompressed_records.push(RawDataRecord { sent_nanos, rtt_nanos });
    }
    decompressed_records
}

// --- Metrics ---

fn print_metrics(
    input_path: &PathBuf,
    compressed_records: &[CompressedDataRecord],
    decompressed_records: &[RawDataRecord],
) -> Result<()> {
    let original_size = 16 + std::mem::size_of_val(decompressed_records);
    let compressed_size = 16 + std::mem::size_of_val(compressed_records);
    let compression_ratio = original_size as f64 / compressed_size as f64;

    println!("\n--- Verification Metrics ---");
    println!("Original size:    {original_size} bytes");
    println!("Compressed size:  {compressed_size} bytes");
    println!("Compression ratio: {compression_ratio:.2}:1");

    let decompressed_map: HashMap<u64, u64> = decompressed_records
        .iter()
        .map(|r| (r.sent_nanos, r.rtt_nanos))
        .collect();

    let (_header, original_records_iterator) = read_raw_records(input_path)?;
    let mut squared_error_sum = 0.0;
    let mut valid_rtt_count = 0;

    for original_result in original_records_iterator {
        let original = original_result?;
        if let Some(decompressed_rtt_nanos) = decompressed_map.get(&original.sent_nanos) {
            if original.rtt_nanos == u64::MAX {
                continue;
            }
            let original_rtt_micros = original.rtt_nanos / 1000;
            if original_rtt_micros >= (u16::MAX - 1) as u64 {
                continue;
            }
            let decompressed_rtt_micros = decompressed_rtt_nanos / 1000;
            let error = original_rtt_micros as i64 - decompressed_rtt_micros as i64;
            squared_error_sum += (error * error) as f64;
            valid_rtt_count += 1;
        }
    }

    if valid_rtt_count > 0 {
        let mean_squared_error = squared_error_sum / valid_rtt_count as f64;
        let root_mean_squared_error = mean_squared_error.sqrt();
        println!("RTT Precision Loss (RMSE): {root_mean_squared_error:.2} microseconds");
    } else {
        println!("RTT Precision Loss (RMSE): Not applicable (no valid RTTs to compare).");
    }

    Ok(())
}
