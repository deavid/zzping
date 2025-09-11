use clap::Parser;

/// A high-frequency ICMP pinger that sends results to a zzping-database server.
#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Path to the collector configuration file.
    #[arg(short, long, default_value = "collector.ron")]
    pub config: String,
}
