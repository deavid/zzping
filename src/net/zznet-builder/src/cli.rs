//! Standard CLI argument parsing for ZZNet applications.
//!
//! This module provides a standardized command-line interface that all ZZNet
//! applications can use, ensuring consistency across the ecosystem.

use clap::Parser;

/// Standard command-line arguments for ZZNet applications.
///
/// All ZZNet applications should accept these standard arguments to provide
/// a consistent user experience. Applications can extend these arguments
/// by embedding this struct in a larger CLI structure.
///
/// # Example
///
/// ```rust,no_run
/// use clap::Parser;
/// use zznet_builder::cli::StandardCliArgs;
///
/// #[derive(Parser)]
/// struct MyAppArgs {
///     #[command(flatten)]
///     standard: StandardCliArgs,
///
///     // Add application-specific args here
///     #[arg(long)]
///     custom_option: Option<String>,
/// }
///
/// let args = MyAppArgs::parse();
/// println!("Config file: {}", args.standard.config);
/// ```
#[derive(Parser, Debug, Clone)]
pub struct StandardCliArgs {
    /// Path to configuration file
    #[arg(short, long, default_value = "config.ron")]
    pub config: String,

    /// Enable debug logging
    #[arg(short, long)]
    pub debug: bool,

    /// Enable trace logging (very verbose)
    #[arg(short, long)]
    pub trace: bool,
}

impl StandardCliArgs {
    /// Get the appropriate log level based on flags.
    ///
    /// Returns "trace" if --trace is set, "debug" if --debug is set,
    /// otherwise "info".
    pub fn log_level(&self) -> &'static str {
        if self.trace {
            "trace"
        } else if self.debug {
            "debug"
        } else {
            "info"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_level_defaults_to_info() {
        let args = StandardCliArgs {
            config: "test.ron".to_string(),
            debug: false,
            trace: false,
        };
        assert_eq!(args.log_level(), "info");
    }

    #[test]
    fn test_log_level_debug() {
        let args = StandardCliArgs {
            config: "test.ron".to_string(),
            debug: true,
            trace: false,
        };
        assert_eq!(args.log_level(), "debug");
    }

    #[test]
    fn test_log_level_trace_overrides_debug() {
        let args = StandardCliArgs {
            config: "test.ron".to_string(),
            debug: true,
            trace: true,
        };
        assert_eq!(args.log_level(), "trace");
    }
}
