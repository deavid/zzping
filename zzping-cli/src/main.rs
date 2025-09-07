use anyhow::Result;
use chrono::DateTime;
use clap::Parser;
use std::path::PathBuf;
use zzping_lib::chunked_v1::decompress_chunked_v1;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Parser, Debug)]
enum Commands {
    /// Inspects a .zzp1 data file and prints a summary.
    InspectFile {
        /// The path to the .zzp1 file to inspect.
        #[arg(required = true)]
        path: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .init();
    let cli = Cli::parse();

    match cli.command {
        Commands::InspectFile { path } => {
            inspect_file(&path)?;
        }
    }

    Ok(())
}

fn inspect_file(path: &PathBuf) -> Result<()> {
    println!("Inspecting file: {}", path.display());

    let file_content = std::fs::read(path)?;
    let records = decompress_chunked_v1(&file_content)?;

    if records.is_empty() {
        println!("File contains no records.");
        return Ok(());
    }

    // --- Header Info ---
    let first_record_time =
        DateTime::from_timestamp_nanos(records.first().unwrap().sent_nanos as i64);
    let last_record_time =
        DateTime::from_timestamp_nanos(records.last().unwrap().sent_nanos as i64);
    println!("\n--- Header Information ---");
    println!("Start Time: {first_record_time}");
    println!("End Time:   {last_record_time}");
    println!("Duration:   {}", last_record_time - first_record_time);

    // --- Statistics ---
    let total_pings = records.len();
    let lost_packets = records.iter().filter(|r| r.rtt_nanos == u64::MAX).count();
    let successful_pings = total_pings - lost_packets;
    let packet_loss_pct = if total_pings > 0 {
        (lost_packets as f64 / total_pings as f64) * 100.0
    } else {
        0.0
    };

    let mut rtts: Vec<u64> = records
        .iter()
        .filter(|r| r.rtt_nanos != u64::MAX)
        .map(|r| r.rtt_nanos)
        .collect();
    rtts.sort_unstable();

    let min_rtt = rtts.first().cloned().unwrap_or(0) as f64 / 1_000_000.0;
    let max_rtt = rtts.last().cloned().unwrap_or(0) as f64 / 1_000_000.0;
    let median_rtt = if successful_pings > 0 {
        rtts[successful_pings / 2] as f64 / 1_000_000.0
    } else {
        0.0
    };

    println!("\n--- Statistics ---");
    println!("Total Pings:      {total_pings}");
    println!("Successful Pings: {successful_pings}");
    println!(
        "Lost Packets:     {lost_packets} ({packet_loss_pct:.2}%)"
    );
    println!("Min RTT:          {min_rtt:.3} ms");
    println!("Median RTT:       {median_rtt:.3} ms");
    println!("Max RTT:          {max_rtt:.3} ms");

    // --- Continuity Check ---
    println!("\n--- Continuity Check ---");
    let mut gaps = 0;
    for i in 1..records.len() {
        let prev = &records[i - 1];
        let curr = &records[i];
        let diff = curr.sent_nanos - prev.sent_nanos;
        // A gap is > 2s, assuming a 1s ping interval for this check.
        if diff > 2_000_000_000 {
            gaps += 1;
            let prev_time = DateTime::from_timestamp_nanos(prev.sent_nanos as i64);
            let curr_time = DateTime::from_timestamp_nanos(curr.sent_nanos as i64);
            println!(
                "Gap detected: {:.3}s between {} and {}",
                diff as f64 / 1_000_000_000.0,
                prev_time,
                curr_time
            );
        }
    }
    if gaps == 0 {
        println!("No significant time gaps found.");
    }

    // --- Duplicate Check ---
    println!("\n--- Duplicate Check ---");
    let mut duplicates = 0;
    for i in 1..records.len() {
        if records[i].sent_nanos == records[i - 1].sent_nanos {
            duplicates += 1;
            println!("Duplicate timestamp found: {}", records[i].sent_nanos);
        }
    }
    if duplicates == 0 {
        println!("No duplicate timestamps found.");
    }

    Ok(())
}
