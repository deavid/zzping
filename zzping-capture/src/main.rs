use chrono::Utc;
use clap::Parser;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::fs::{self, File, OpenOptions};
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::signal;
use tokio::time::{Instant, MissedTickBehavior};

const MAGIC_NUMBER: u64 = 0x7A7A504E47434150; // "zzPNG CAP"

/// A high-frequency ICMP pinger that captures raw results.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// The IP address to ping
    #[arg(long)]
    target: IpAddr,

    /// The number of pings to send per second
    #[arg(long)]
    rate: u64,

    /// The directory where log files will be saved
    #[arg(long, default_value = "./capture_logs/")]
    output_dir: PathBuf,
}

struct LogFile {
    writer: BufWriter<File>,
    start_time_monotonic: Instant,
}

impl LogFile {
    async fn new(
        dir: &Path,
        target: IpAddr,
        date_str: &str,
    ) -> Result<Self, std::io::Error> {
        let file_path = dir.join(format!("{target}-{date_str}.dat"));
        eprintln!("Creating new log file: {}", file_path.display());

        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&file_path)
            .await?;

        let mut writer = BufWriter::new(file);

        // Header is always written for a new file now.
        let start_time_system = SystemTime::now();
        let start_time_unix_nanos = start_time_system
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("Time went backwards")
            .as_nanos() as u64;

        writer.write_u64_le(MAGIC_NUMBER).await?;
        writer.write_u64_le(start_time_unix_nanos).await?;

        Ok(LogFile {
            writer,
            start_time_monotonic: Instant::now(),
        })
    }

    async fn write_record(&mut self, sent_nanos: u64, rtt_nanos: u64) -> Result<(), std::io::Error> {
        self.writer.write_u64_le(sent_nanos).await?;
        self.writer.write_u64_le(rtt_nanos).await?;
        Ok(())
    }

    async fn flush(&mut self) -> Result<(), std::io::Error> {
        self.writer.flush().await
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    fs::create_dir_all(&cli.output_dir).await?;

    eprintln!("Starting capture for target: {}", cli.target);
    eprintln!("Pinging at {} pps", cli.rate);
    eprintln!("Output directory: {}", cli.output_dir.display());

    let interval_duration = Duration::from_secs_f64(1.0 / cli.rate as f64);
    let mut interval = tokio::time::interval(interval_duration);
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

    let target_ip = cli.target;
    let data_arc = Arc::new(&[][..]);
    let timeout = Duration::from_secs(2);

    let mut current_log_file: Option<LogFile> = None;
    let mut last_date_str = String::new();

    loop {
        let _ = tokio::select! {
            biased;
            _ = signal::ctrl_c() => {
                eprintln!("\nShutdown signal received. Exiting...");
                break;
            }
            instant = interval.tick() => instant,
        };

        let now = Utc::now();
        let date_str = now.format("%Y%m%d").to_string();

        if last_date_str != date_str {
            if let Some(mut log_file) = current_log_file.take() {
                log_file.flush().await?;
            }
            let log_file = LogFile::new(&cli.output_dir, target_ip, &date_str).await?;
            current_log_file = Some(log_file);
            last_date_str = date_str;
        }

        if let Some(log_file) = current_log_file.as_mut() {
            let sent_instant = Instant::now();
            let sent_nanos = sent_instant
                .duration_since(log_file.start_time_monotonic)
                .as_nanos() as u64;

            let future = ping_rs::send_ping_async(&target_ip, timeout, data_arc.clone(), None);
            let rtt_nanos = match future.await {
                Ok(reply) => (reply.rtt as u64) * 1_000_000,
                Err(_) => u64::MAX,
            };

            log_file.write_record(sent_nanos, rtt_nanos).await?;
        }
    }

    if let Some(mut log_file) = current_log_file.take() {
        log_file.flush().await?;
    }

    eprintln!("Capture finished.");
    Ok(())
}
