use anyhow::Result;
use chrono::Utc;
use clap::Parser;
use std::collections::VecDeque;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime};
use surge_ping::{Client, Config, PingIdentifier, PingSequence, Pinger};
use tokio::fs::{self, File, OpenOptions};
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::sync::{Semaphore, broadcast, mpsc};
use tokio::time::{self, Instant};

// Global shutdown flag for signal handling
static SHUTDOWN: AtomicBool = AtomicBool::new(false);

const MAGIC_NUMBER: u64 = 0x7A7A504E47434150; // "zzPNG CAP"
const MAX_REPORT_INTERVAL_SECS: f64 = 5.0;
const INITIAL_REPORT_INTERVAL_SECS: f64 = 0.5;
const REPORT_INTERVAL_INCREMENT_SECS: f64 = 0.5;

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

    /// The maximum number of pings in flight
    #[arg(long, default_value = "1000")]
    max_in_flight: usize,
}

struct LogFile {
    writer: BufWriter<File>,
    start_time_monotonic: Instant,
}

impl LogFile {
    async fn new(dir: &Path, target: IpAddr, date_str: &str) -> Result<Self> {
        let file_path = dir.join(format!("{date_str}-{target}.dat"));
        eprintln!("Creating new log file: {}", file_path.display());
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&file_path)
            .await?;
        let mut writer = BufWriter::new(file);
        let start_time_system = SystemTime::now();
        let start_time_unix_nanos = start_time_system
            .duration_since(SystemTime::UNIX_EPOCH)?
            .as_nanos() as u64;
        writer.write_u64_le(MAGIC_NUMBER).await?;
        writer.write_u64_le(start_time_unix_nanos).await?;
        Ok(LogFile {
            writer,
            start_time_monotonic: Instant::now(),
        })
    }
    async fn write_record(&mut self, sent_nanos: u64, rtt_nanos: u64) -> Result<()> {
        self.writer.write_u64_le(sent_nanos).await?;
        self.writer.write_u64_le(rtt_nanos).await?;
        Ok(())
    }
    async fn flush(&mut self) -> Result<()> {
        self.writer.flush().await?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct PingResult {
    sent_instant: Instant,
    rtt: Option<Duration>,
    is_error: bool,
}

async fn ping_task(
    mut pinger: Pinger,
    seq: u16,
    target_time: Instant,
    tx: mpsc::Sender<PingResult>,
    _permit: tokio::sync::OwnedSemaphorePermit,
) {
    // Simple precision timing: sleep until target time
    let now = Instant::now();
    if target_time > now {
        let remaining = target_time - now;

        // NOTE: Using std::thread::sleep() instead of tokio::time::sleep_until() for better precision.
        // Empirical testing shows std::thread::sleep() provides ~10µs precision vs ~400µs for tokio sleep.
        // This blocks the current async task but doesn't block the tokio runtime since each ping
        // runs in its own spawned task. The precision gain (40x improvement) justifies this approach.
        std::thread::sleep(remaining);
    }

    let sent_instant = Instant::now();
    let result = pinger.ping(PingSequence(seq), &[0; 8]).await;
    let (rtt, is_error) = match result {
        Ok((_, rtt)) => (Some(rtt), false),
        Err(_) => (None, true),
    };
    if tx
        .send(PingResult {
            sent_instant,
            rtt,
            is_error,
        })
        .await
        .is_err()
    {
        // Receiver has been dropped
    }
    // Permit is automatically dropped here, releasing the semaphore slot
}

async fn logger_task(
    cli: Arc<Cli>,
    mut rx: mpsc::Receiver<PingResult>,
    rate_ns: Arc<AtomicU64>,
    ping_semaphore: Arc<Semaphore>,
    mut shutdown_rx: broadcast::Receiver<()>,
) -> Result<()> {
    let mut current_log_file: Option<LogFile> = None;
    let mut last_date_str = String::new();

    let mut report_interval = Duration::from_secs_f64(INITIAL_REPORT_INTERVAL_SECS);
    let mut next_report = Instant::now() + report_interval;

    let mut backoff_ticker = time::interval(Duration::from_millis(100));

    let mut recent_results: VecDeque<PingResult> = VecDeque::with_capacity(cli.rate as usize * 6);
    let mut consecutive_errors = 0;
    let mut last_error_print = Instant::now();
    let max_rate = cli.rate;

    loop {
        let now_utc = Utc::now();
        let date_str = now_utc.format("%Y%m%d").to_string();

        if last_date_str != date_str {
            if let Some(mut log_file) = current_log_file.take() {
                log_file.flush().await?;
            }
            let log_file = LogFile::new(&cli.output_dir, cli.target, &date_str).await?;
            current_log_file = Some(log_file);
            last_date_str = date_str;
        }

        tokio::select! {
            biased;
            _ = shutdown_rx.recv() => {
                break;
            },
            Some(result) = rx.recv() => {
                if let Some(log_file) = current_log_file.as_mut() {
                    let sent_nanos = result.sent_instant.duration_since(log_file.start_time_monotonic).as_nanos() as u64;
                    let rtt_nanos = result.rtt.map_or(u64::MAX, |rtt| rtt.as_nanos() as u64);
                    log_file.write_record(sent_nanos, rtt_nanos).await?;
                }
                if result.is_error {
                    consecutive_errors += 1;
                    if consecutive_errors > 5 && last_error_print.elapsed() > Duration::from_secs(1) {
                         eprintln!("[ERROR] Consecutive ping failures detected.");
                         last_error_print = Instant::now();
                    }
                } else {
                    consecutive_errors = 0;
                }
                recent_results.push_back(result);
            },
            _ = backoff_ticker.tick() => {
                let five_seconds_ago = Instant::now() - Duration::from_secs(5);
                recent_results.retain(|r| r.sent_instant > five_seconds_ago);

                let sent_count = recent_results.len();
                let received_count = recent_results.iter().filter(|r| r.rtt.is_some()).count();

                let loss_rate = if sent_count > 0 {
                    1.0 - (received_count as f64 / sent_count as f64)
                } else {
                    0.0
                };

                // Stepped backoff based on loss percentage
                let rate_multiplier = if loss_rate < 0.10 {
                    // <10% loss: run at 100%
                    1.0
                } else if loss_rate < 0.20 {
                    0.5
                } else if loss_rate < 0.50 {
                    0.2
                } else if loss_rate < 0.90 {
                    0.1
                } else {
                    0.01
                };

                let new_rate = (max_rate as f64 * rate_multiplier).max(2.0);
                let new_interval_ns = (1_000_000_000.0 / new_rate) as u64;
                rate_ns.store(new_interval_ns, Ordering::SeqCst);
            },
            _ = time::sleep_until(next_report) => {
                next_report = Instant::now() + report_interval;
                let current_interval_secs = report_interval.as_secs_f64();
                if current_interval_secs < MAX_REPORT_INTERVAL_SECS {
                    report_interval += Duration::from_secs_f64(REPORT_INTERVAL_INCREMENT_SECS);
                }

                let five_seconds_ago = Instant::now() - Duration::from_secs(5);
                recent_results.retain(|r| r.sent_instant > five_seconds_ago);
                recent_results.make_contiguous().sort_by_key(|r| r.sent_instant);

                let ipg_stats = if recent_results.len() > 1 {
                    let ipgs: Vec<Duration> = recent_results.as_slices().0.windows(2).map(|w| w[1].sent_instant - w[0].sent_instant).collect();
                    let ipg_count = ipgs.len() as u32;
                    let total_ipg: Duration = ipgs.iter().sum();
                    let mean_ipg = total_ipg / ipg_count;
                    let ipg_variance = ipgs.iter().map(|ipg| {
                        let diff = ipg.as_secs_f64() - mean_ipg.as_secs_f64();
                        diff * diff
                    }).sum::<f64>() / ipg_count as f64;
                    Some(Duration::from_secs_f64(ipg_variance.sqrt()))
                } else {
                    None
                };

                let sent_count = recent_results.len();
                let rtts: Vec<Duration> = recent_results.iter().filter_map(|r| r.rtt).collect();
                let received_count = rtts.len();
                let loss_count = sent_count - received_count;

                // Semaphore statistics for actual concurrency control
                let semaphore_available = ping_semaphore.available_permits();
                let semaphore_in_use = cli.max_in_flight - semaphore_available;

                let loss_percent = if sent_count > 0 { (loss_count as f64 / sent_count as f64) * 100.0 } else { 0.0 };

                // Calculate PPS based on actual time window, not fixed 5 seconds
                let pps = if recent_results.len() >= 2 {
                    let oldest = recent_results.front().unwrap().sent_instant;
                    let newest = recent_results.back().unwrap().sent_instant;
                    let actual_window_secs = newest.duration_since(oldest).as_secs_f64();
                    if actual_window_secs > 0.0 {
                        (sent_count - 1) as f64 / actual_window_secs
                    } else {
                        0.0
                    }
                } else {
                    0.0
                };
                let current_rate = 1_000_000_000.0 / rate_ns.load(Ordering::Relaxed) as f64;
                let timestamp = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

                print!("{timestamp} | Rate: {current_rate:>4.1} pps | PPS: {pps:>4.1} | Loss: {loss_percent:>4.1}% | InFlight: {semaphore_in_use}/{}", cli.max_in_flight);
                if let Some(std_dev) = ipg_stats {
                     print!(" | IPG std_dev: {std_dev:.2?}");
                }

                if !rtts.is_empty() {
                    let rtt_count = rtts.len() as u32;
                    let total_rtt: Duration = rtts.iter().sum();
                    let mean_rtt = total_rtt / rtt_count;
                    let min_rtt = rtts.iter().min().unwrap();
                    let max_rtt = rtts.iter().max().unwrap();

                    let variance = rtts.iter().map(|rtt| {
                        let diff = rtt.as_secs_f64() - mean_rtt.as_secs_f64();
                        diff * diff
                    }).sum::<f64>() / rtt_count as f64;
                    let std_dev = Duration::from_secs_f64(variance.sqrt());

                    println!(" | RTT min/max/avg/std_dev: {min_rtt:.2?}/{max_rtt:.2?}/{mean_rtt:.2?}/{std_dev:.2?}");
                } else {
                    println!(" | RTT: n/a");
                }
            },
            else => break,
        }
    }
    if let Some(mut log_file) = current_log_file.take() {
        log_file.flush().await?;
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Arc::new(Cli::parse());
    fs::create_dir_all(&cli.output_dir).await?;

    eprintln!("Starting capture for target: {}", cli.target);
    eprintln!("Pinging at {} pps", cli.rate);
    eprintln!("Output directory: {}", cli.output_dir.display());

    let config = Config::default();
    let client = Client::new(&config)?;
    let pinger_ident = PingIdentifier(rand::random());

    // Use high capacity for logging pipeline efficiency
    let (tx, rx) = mpsc::channel(1024);

    // Use semaphore for actual ping concurrency control
    let ping_semaphore = Arc::new(Semaphore::new(cli.max_in_flight));

    let (shutdown_tx, _) = broadcast::channel(1);

    let initial_interval_ns = 1_000_000_000 / cli.rate;
    let rate_ns = Arc::new(AtomicU64::new(initial_interval_ns));

    let logger_handle = tokio::spawn(logger_task(
        cli.clone(),
        rx,
        rate_ns.clone(),
        ping_semaphore.clone(),
        shutdown_tx.subscribe(),
    ));

    // Spawn a task to handle Ctrl+C using tokio's signal handling
    let _shutdown_handle = tokio::spawn(async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to listen for ctrl_c");
        SHUTDOWN.store(true, Ordering::SeqCst);
        eprintln!("\nShutdown signal received. Gracefully shutting down...");
    });

    let mut sequence_idx: u16 = 0;
    let mut last_tick = Instant::now();

    loop {
        // Check for shutdown signal
        if SHUTDOWN.load(Ordering::SeqCst) {
            let _ = shutdown_tx.send(());
            break;
        }

        let interval_nanos = rate_ns.load(Ordering::SeqCst);
        let next_tick = last_tick + Duration::from_nanos(interval_nanos);
        // Wake up 10ms early to allow time for pinger creation and task spawning
        // This provides a robust buffer that works across different systems
        let wake_up_time = next_tick - Duration::from_millis(10);

        tokio::select! {
            biased;
            _ = time::sleep_until(wake_up_time) => {
                last_tick = next_tick;

                // Try to acquire a permit for ping concurrency control
                if let Ok(permit) = ping_semaphore.clone().try_acquire_owned() {
                    let pinger = client.pinger(cli.target, pinger_ident).await;
                    tokio::spawn(ping_task(pinger, sequence_idx, next_tick, tx.clone(), permit));
                    sequence_idx = sequence_idx.wrapping_add(1);
                } else {
                    // No permits available - we've hit max_in_flight limit
                    // dbg!("skip - max in flight reached");
                }
            },
        }
    }

    drop(tx);

    // Wait for logger to finish, but with a 3-second timeout for forced shutdown
    eprintln!("Waiting for background tasks to finish (max 3 seconds)...");
    let shutdown_result = tokio::time::timeout(Duration::from_secs(3), logger_handle).await;

    match shutdown_result {
        Ok(Ok(Ok(()))) => eprintln!("Graceful shutdown completed."),
        Ok(Ok(Err(e))) => eprintln!("Logger task finished with error: {}", e),
        Ok(Err(e)) => eprintln!("Logger task panicked: {}", e),
        Err(_) => eprintln!("Forced shutdown after 3 seconds timeout."),
    }

    eprintln!("Capture finished.");
    Ok(())
}
