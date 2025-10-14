//! Command-line argument parsing.

use clap::Parser;

/// ZZPing Collector - Network monitoring client
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct CliArgs {
    /// Path to configuration file
    #[arg(short, long, default_value = "collector.ron")]
    pub config: String,

    /// Enable debug logging
    #[arg(short, long)]
    pub debug: bool,

    /// Enable trace logging (very verbose)
    #[arg(short, long)]
    pub trace: bool,
}
