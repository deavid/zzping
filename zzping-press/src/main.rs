use anyhow::{Context, Result, bail};
use clap::Parser;
use rerun::RecordingStreamBuilder;
use std::fs::File;
use std::io::{BufReader, Read};
use std::marker::PhantomData;
use std::path::PathBuf;
use std::time::Duration;

const CAPTURE_MAGIC: u64 = 0x7A7A504E47434150; // zzPNGCAP

// --- Data Structs & I/O Infrastructure ---

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RawDataRecord {
    sent_nanos: u64,
    rtt_nanos: u64,
}

trait FromBytes: Sized {
    const SIZE: usize;
    fn from_le_bytes(bytes: &[u8]) -> Result<Self>;
}

impl FromBytes for RawDataRecord {
    const SIZE: usize = 16;
    fn from_le_bytes(bytes: &[u8]) -> Result<Self> {
        let sent_nanos = u64::from_le_bytes(bytes[0..8].try_into()?);
        let rtt_nanos = u64::from_le_bytes(bytes[8..16].try_into()?);
        Ok(Self {
            sent_nanos,
            rtt_nanos,
        })
    }
}

struct RecordIterator<R: Read, T: FromBytes> {
    reader: R,
    _phantom: PhantomData<T>,
}

impl<R: Read, T: FromBytes> RecordIterator<R, T> {
    fn new(reader: R) -> Self {
        Self {
            reader,
            _phantom: PhantomData,
        }
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

fn read_raw_records(path: &PathBuf) -> Result<RecordIterator<BufReader<File>, RawDataRecord>> {
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
    Ok(RecordIterator::new(reader))
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
    // For now, focusing on the Rerun implementation. DeltaQuantizedV1 will be re-added later.
    // DeltaQuantizedV1,
    /// Output the data to a Rerun RRD file for visualization.
    Rerun,
}

// --- Main Application Logic ---

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.strategy {
        Strategy::Rerun => {
            // --- Rerun Export Implementation ---
            // This block handles the full process of converting a raw zzping data file
            // into a Rerun RRD file for visualization.

            // 1. Initialize the Rerun RecordingStream.
            //    The `save()` method configures the stream to write all subsequent
            //    log data directly to the specified `.rrd` file. This is more
            //    efficient than buffering in memory.
            let rec = RecordingStreamBuilder::new("zzping-press").save(&cli.output)?;

            // 2. Read all raw records into memory.
            //    This is necessary because the data must be sorted by timestamp
            //    to be displayed correctly as a time-series plot.
            let raw_records_iterator = read_raw_records(&cli.input)?;
            let mut records: Vec<RawDataRecord> = raw_records_iterator.collect::<Result<_>>()?;
            records.sort_by_key(|r| r.sent_nanos);

            println!("Logging {} records to Rerun file...", records.len());

            // 3. Iterate through sorted records and log them to Rerun.
            for record in records {
                // RTT is meaningless for lost packets, so we skip them.
                if record.rtt_nanos == u64::MAX {
                    continue;
                }

                // 3a. Set the time for the following log calls.
                //     We define a custom timeline named "sent_time" and use the
                //     monotonic `sent_nanos` from our data as the time value.
                //     This ensures the x-axis of our plot is the send time.
                rec.set_time("sent_time", Duration::from_nanos(record.sent_nanos));

                // 3b. Log the RTT as a scalar value.
                //     We give it the entity path "ping/rtt_ms", which will create a
                //     plot named "rtt_ms" inside a "ping" group in the Rerun UI.
                //     We convert the RTT to milliseconds for readability.
                let rtt_ms = record.rtt_nanos as f64 / 1_000_000.0;
                rec.log("ping/rtt_ms", &rerun::Scalars::new([rtt_ms]))?;
            }

            println!("Successfully wrote Rerun data to {}.", cli.output.display());

            // 4. Report the final file size as requested.
            let metadata = std::fs::metadata(&cli.output)?;
            println!("Final RRD file size: {} bytes", metadata.len());
        }
    }

    Ok(())
}
