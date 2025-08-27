use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use rerun::RecordingStreamBuilder;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::PathBuf;
use std::time::Duration;
use zzping_press::{
    chunked_v1, CaptureHeader, CompressedDataRecord, RawDataRecord, RecordIterator,
};

const CAPTURE_MAGIC: u64 = 0x7A7A504E47434150; // zzPNGCAP
const PRESS_MAGIC_V1: u64 = 0x7A7A505245535331; // zzPRESS1

// --- CLI Definition ---

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Compresses a raw .dat file into a compressed format.
    Press(PressArgs),
    /// Inspects a raw .dat file and prints metadata.
    Inspect(InspectArgs),
    /// Creates a fixture file by slicing a raw .dat file.
    CreateFixture(CreateFixtureArgs),
}

#[derive(Parser, Debug)]
struct PressArgs {
    #[arg(long, value_name = "FILE_PATH")]
    input: PathBuf,
    #[arg(
        long,
        value_name = "FILE_PATH",
        help = "Output file path. If not specified, will add extension based on compression strategy to input file"
    )]
    output: Option<PathBuf>,
    #[arg(
        long,
        value_name = "STRATEGY_NAME",
        default_value = "delta-quantized-v1",
        help = "Compression strategy to use"
    )]
    strategy: Strategy,
}

#[derive(Parser, Debug)]
struct InspectArgs {
    #[arg(long, value_name = "FILE_PATH")]
    input: PathBuf,
}

#[derive(Parser, Debug)]
struct CreateFixtureArgs {
    #[arg(long, value_name = "FILE_PATH")]
    input: PathBuf,
    #[arg(long, value_name = "FILE_PATH")]
    output: PathBuf,
    #[arg(long, help = "Number of minutes to skip from the beginning")]
    skip: u64,
    #[arg(long, help = "Number of minutes to include in the fixture")]
    take: u64,
}

#[derive(clap::ValueEnum, Clone, Debug)]
enum Strategy {
    DeltaQuantizedV1,
    ChunkedV1,
    /// Output the data to a Rerun RRD file for visualization.
    Rerun,
}

impl Strategy {
    fn file_extension(&self) -> &'static str {
        match self {
            Strategy::DeltaQuantizedV1 => "dqv1",
            Strategy::ChunkedV1 => "zzp1",
            Strategy::Rerun => "rrd",
        }
    }
}

fn get_output_path(
    input: &std::path::Path,
    output: Option<PathBuf>,
    strategy: &Strategy,
) -> PathBuf {
    match output {
        Some(path) => path,
        None => {
            let mut output_path = input.to_path_buf();
            let extension = strategy.file_extension();
            output_path.set_extension(extension);
            output_path
        }
    }
}

// --- Main Application Logic ---

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Press(args) => handle_press(args),
        Commands::Inspect(args) => handle_inspect(args),
        Commands::CreateFixture(args) => handle_create_fixture(args),
    }
}

fn handle_press(args: PressArgs) -> Result<()> {
    // Determine the output path
    let output_path = get_output_path(&args.input, args.output, &args.strategy);

    match args.strategy {
        Strategy::ChunkedV1 => {
            let (_header, raw_records_iterator) = read_raw_records(&args.input)?;
            let mut records: Vec<RawDataRecord> = raw_records_iterator.collect::<Result<_>>()?;
            records.sort();

            println!("Compressing {} records with chunked-v1 strategy...", records.len());

            let start_time = std::time::Instant::now();
            let compressed_data = chunked_v1::compress_chunked_v1(&records)?;
            let duration = start_time.elapsed();

            let original_size = records.len() * std::mem::size_of::<RawDataRecord>();
            let compressed_size = compressed_data.len();
            let ratio = if compressed_size > 0 {
                original_size as f64 / compressed_size as f64
            } else {
                0.0
            };
            let records_per_sec = records.len() as f64 / duration.as_secs_f64();

            println!("--- Compression Summary ---");
            println!("Original size: {original_size} bytes");
            println!("Compressed size: {compressed_size} bytes");
            println!("Compression ratio: {ratio:.2}:1");
            println!("Processing speed: {:.2} Million Records/sec", records_per_sec / 1_000_000.0);


            let mut writer = BufWriter::new(File::create(&output_path).with_context(|| {
                format!("Failed to create output file: {}", output_path.display())
            })?);
            writer.write_all(&compressed_data)?;

            println!(
                "\nSuccessfully wrote {} compressed bytes to {}.",
                compressed_data.len(),
                output_path.display()
            );
        }
        Strategy::DeltaQuantizedV1 => {
            // --- Compression Phase ---
            let (header, raw_records_iterator) = read_raw_records(&args.input)?;
            let mut records: Vec<RawDataRecord> = raw_records_iterator.collect::<Result<_>>()?;

            // Debug: Sample first 10 records to verify data makes sense
            println!("Header start timestamp: {} ns", header.start_timestamp_ns);
            println!("Total records read: {}", records.len());
            println!("Sampling first 5 raw records:");
            for (i, record) in records.iter().take(5).enumerate() {
                let sent_us = record.sent_nanos as f64 / 1000.0;
                let rtt_display = if record.rtt_nanos == u64::MAX {
                    "LOST".to_string()
                } else {
                    format!("{:.1} us", record.rtt_nanos as f64 / 1000.0)
                };
                println!("  {i}: sent={sent_us:.1}us, rtt={rtt_display}");
            }

            let mut writer = BufWriter::new(File::create(&output_path).with_context(|| {
                format!("Failed to create output file: {}", output_path.display())
            })?);
            write_press_header(&mut writer, header.start_timestamp_ns)?;

            let records_written = stream_compress_delta_quantized_v1(&mut records, &mut writer)?;
            println!(
                "Successfully wrote {} compressed records to {}.",
                records_written,
                output_path.display()
            );

            // --- Verification Phase ---
            let compressed_records_iterator = read_compressed_records(&output_path)?;
            let read_compressed_records: Vec<_> =
                compressed_records_iterator.collect::<Result<_>>()?;

            // Debug: Sample first 5 compressed records
            println!("Sampling first 5 compressed records:");
            for (i, record) in read_compressed_records.iter().take(5).enumerate() {
                let rtt_display = if record.rtt_deciseconds == u16::MAX {
                    "LOST".to_string()
                } else {
                    format!("{:.1} ms", record.rtt_deciseconds as f64 / 10.0)
                };
                println!(
                    "  {}: delta={:.1}ms, rtt={}",
                    i,
                    record.sent_delta_deciseconds as f64 / 10.0,
                    rtt_display
                );
            }

            let decompressed_records = decompress_delta_quantized_v1(&read_compressed_records);

            print_metrics(&args.input, &read_compressed_records, &decompressed_records)?;
        }
        Strategy::Rerun => {
            // --- Rerun Export Implementation ---
            let rec = RecordingStreamBuilder::new("zzping-press").save(&output_path)?;
            let (_header, raw_records_iterator) = read_raw_records(&args.input)?;
            let mut records: Vec<RawDataRecord> = raw_records_iterator.collect::<Result<_>>()?;
            records.sort_by_key(|r| r.sent_nanos);

            println!("Logging {} records to Rerun file...", records.len());

            for record in records {
                if record.rtt_nanos == u64::MAX {
                    continue;
                }
                rec.set_time("sent_time", Duration::from_nanos(record.sent_nanos));
                let rtt_ms = record.rtt_nanos as f64 / 1_000_000.0;
                rec.log("ping/rtt_ms", &rerun::Scalars::new([rtt_ms]))?;
            }

            println!(
                "Successfully wrote Rerun data to {}.",
                output_path.display()
            );

            let metadata = std::fs::metadata(&output_path)?;
            println!("Final RRD file size: {} bytes", metadata.len());
        }
    }
    Ok(())
}

fn handle_inspect(args: InspectArgs) -> Result<()> {
    let (_header, raw_records_iterator) = read_raw_records(&args.input)?;
    let records: Vec<RawDataRecord> = raw_records_iterator.collect::<Result<_>>()?;

    if records.is_empty() {
        println!("File contains no records.");
        return Ok(());
    }

    let first_ts = records.iter().map(|r| r.sent_nanos).min().unwrap_or(0);
    let last_ts = records.iter().map(|r| r.sent_nanos).max().unwrap_or(0);
    let duration_ns = last_ts - first_ts;
    let duration_secs = duration_ns as f64 / 1_000_000_000.0;
    let duration_mins = duration_secs / 60.0;

    println!("--- Inspecting Raw Data File ---");
    println!("File: {}", args.input.display());
    println!("Total records: {}", records.len());
    println!("Total duration: {duration_secs:.2} seconds ({duration_mins:.2} minutes)");

    Ok(())
}

fn handle_create_fixture(args: CreateFixtureArgs) -> Result<()> {
    let (header, raw_records_iterator) = read_raw_records(&args.input)?;
    let mut records: Vec<RawDataRecord> = raw_records_iterator.collect::<Result<_>>()?;
    records.sort();

    if records.is_empty() {
        bail!("Input file has no records, cannot create fixture.");
    }

    let first_ts = records[0].sent_nanos;
    let skip_ns = args.skip * 60 * 1_000_000_000;
    let take_ns = args.take * 60 * 1_000_000_000;

    let start_ts = first_ts + skip_ns;
    let end_ts = start_ts + take_ns;

    let fixture_records: Vec<_> = records
        .into_iter()
        .filter(|r| r.sent_nanos >= start_ts && r.sent_nanos < end_ts)
        .collect();

    if let Some(parent) = args.output.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut writer = BufWriter::new(File::create(&args.output).with_context(|| {
        format!("Failed to create output file: {}", args.output.display())
    })?);

    // Write header
    writer.write_all(&CAPTURE_MAGIC.to_le_bytes())?;
    writer.write_all(&header.start_timestamp_ns.to_le_bytes())?;

    // Write records
    for record in &fixture_records {
        writer.write_all(&record.sent_nanos.to_le_bytes())?;
        writer.write_all(&record.rtt_nanos.to_le_bytes())?;
    }

    println!("--- Creating Fixture ---");
    println!("Input: {}", args.input.display());
    println!("Output: {}", args.output.display());
    println!("Skipped {} minutes, Took {} minutes.", args.skip, args.take);
    println!("Wrote {} records to fixture file.", fixture_records.len());

    Ok(())
}


// --- File I/O Functions ---

fn read_raw_records(
    path: &PathBuf,
) -> Result<(
    CaptureHeader,
    RecordIterator<BufReader<File>, RawDataRecord>,
)> {
    let file = File::open(path)
        .with_context(|| format!("Failed to open input file: {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut header_buf = [0u8; 16];
    reader
        .read_exact(&mut header_buf)
        .context("Failed to read capture header")?;
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
    reader
        .read_exact(&mut header_buf)
        .context("Failed to read press header")?;
    let magic = u64::from_le_bytes(header_buf[0..8].try_into()?);
    if magic != PRESS_MAGIC_V1 {
        bail!(
            "Invalid magic number in compressed file. Expected {PRESS_MAGIC_V1:x}, found {magic:x}"
        );
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
        let sent_delta_deciseconds_f64 = sent_delta_nanos as f64 / 100_000.0; // Convert to deciseconds (0.1ms)
        let sent_delta_deciseconds =
            (sent_delta_deciseconds_f64.round() as u64).min(u16::MAX as u64) as u16;
        last_sent_nanos = record.sent_nanos;
        let rtt_deciseconds = if record.rtt_nanos == u64::MAX {
            u16::MAX // Packet lost
        } else {
            let rtt_deciseconds_f64 = record.rtt_nanos as f64 / 100_000.0; // Convert to deciseconds
            let rtt_ds = rtt_deciseconds_f64.round() as u64;
            if rtt_ds >= (u16::MAX - 100) as u64 {
                u16::MAX - 100 // Too long, but not packet lost - reserve top 100 values
            } else {
                rtt_ds as u16
            }
        };
        writer.write_all(&sent_delta_deciseconds.to_le_bytes())?;
        writer.write_all(&rtt_deciseconds.to_le_bytes())?;
    }
    Ok(records.len())
}

fn decompress_delta_quantized_v1(
    compressed_records: &[CompressedDataRecord],
) -> Vec<RawDataRecord> {
    let mut decompressed_records = Vec::with_capacity(compressed_records.len());
    let mut last_sent_nanos: u64 = 0;
    for compressed in compressed_records {
        let sent_nanos =
            last_sent_nanos.saturating_add(compressed.sent_delta_deciseconds as u64 * 100_000);
        last_sent_nanos = sent_nanos;
        let rtt_nanos = if compressed.rtt_deciseconds == u16::MAX {
            u64::MAX // Packet lost
        } else {
            compressed.rtt_deciseconds as u64 * 100_000 // Convert deciseconds back to nanoseconds
        };
        decompressed_records.push(RawDataRecord {
            sent_nanos,
            rtt_nanos,
        });
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

    // Calculate MiB/day based on actual time span in the data
    let mut mib_per_day_text = String::new();
    if !decompressed_records.is_empty() {
        let first_timestamp = decompressed_records
            .iter()
            .map(|r| r.sent_nanos)
            .min()
            .unwrap_or(0);
        let last_timestamp = decompressed_records
            .iter()
            .map(|r| r.sent_nanos)
            .max()
            .unwrap_or(0);
        let time_span_seconds = (last_timestamp - first_timestamp) as f64 / 1_000_000_000.0;

        if time_span_seconds > 0.0 {
            let records_per_second = decompressed_records.len() as f64 / time_span_seconds;
            let records_per_day = records_per_second * 86400.0;
            let original_mib_per_day = (records_per_day * 16.0 + 16.0) / (1024.0 * 1024.0);
            let compressed_mib_per_day = (records_per_day * 4.0 + 16.0) / (1024.0 * 1024.0);

            mib_per_day_text = format!(
                "\nOriginal storage:    {original_mib_per_day:.2} MiB/day\nCompressed storage:  {compressed_mib_per_day:.2} MiB/day"
            );
        }
    }

    println!("\n--- Verification Metrics ---");
    println!("Original size:    {original_size} bytes");
    println!("Compressed size:  {compressed_size} bytes");
    println!("Compression ratio: {compression_ratio:.2}:1{mib_per_day_text}");

    // Read original records to compare by order, not by timestamp
    let (_header, original_records_iterator) = read_raw_records(input_path)?;
    let mut original_records: Vec<_> = original_records_iterator.collect::<Result<Vec<_>>>()?;

    // Sort both datasets by timestamp to ensure same ordering for comparison
    original_records.sort_by_key(|r| r.sent_nanos);
    // Convert to Vec and sort decompressed records to match
    let mut decompressed_sorted = decompressed_records.to_vec();
    decompressed_sorted.sort_by_key(|r| r.sent_nanos);

    // Now both are in the same order, so compare by index
    let mut squared_error_sum = 0.0;
    let mut valid_rtt_count = 0;
    let mut skipped_lost_packets = 0;
    let mut skipped_overflow = 0;

    let min_len = original_records.len().min(decompressed_sorted.len());

    for i in 0..min_len {
        let original = &original_records[i];
        let decompressed = &decompressed_sorted[i];

        // Skip lost packets (RTT = u64::MAX)
        if original.rtt_nanos == u64::MAX {
            skipped_lost_packets += 1;
            continue;
        }

        // Skip RTTs that would overflow u16 when compressed (using deciseconds now)
        let original_rtt_deciseconds_f64 = original.rtt_nanos as f64 / 100_000.0;
        if original_rtt_deciseconds_f64 >= (u16::MAX - 100) as f64 {
            skipped_overflow += 1;
            continue;
        }

        // Compare at nanosecond level using f64 for all statistical calculations
        let original_rtt_nanos_f64 = original.rtt_nanos as f64;
        let decompressed_rtt_nanos_f64 = decompressed.rtt_nanos as f64;
        let error_nanos_f64 = original_rtt_nanos_f64 - decompressed_rtt_nanos_f64;

        // Debug: Print some errors to see what's happening
        if valid_rtt_count < 3 {
            println!(
                "  Sample {}: orig_rtt={}ns, decomp_rtt={}ns, error={:.1}ns",
                valid_rtt_count, original.rtt_nanos, decompressed.rtt_nanos, error_nanos_f64
            );
        }

        squared_error_sum += error_nanos_f64 * error_nanos_f64;
        valid_rtt_count += 1;
    }

    println!(
        "RMSE Debug: total_records={min_len}, valid_rtt_count={valid_rtt_count}, skipped_lost={skipped_lost_packets}, skipped_overflow={skipped_overflow}"
    );

    if valid_rtt_count > 0 {
        let mean_squared_error = squared_error_sum / valid_rtt_count as f64;
        let root_mean_squared_error = mean_squared_error.sqrt();
        println!("RTT Precision Loss (RMSE): {root_mean_squared_error:.2} nanoseconds");
    } else {
        println!("RTT Precision Loss (RMSE): Not applicable (no valid RTTs to compare).");
    }

    Ok(())
}
