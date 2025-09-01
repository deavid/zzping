use clap::Parser;
use std::net::IpAddr;

/// A high-frequency ICMP pinger that sends results to a zzping-database server.
#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// The IP addresses to ping. Multiple targets can be specified.
    #[arg(long)]
    pub targets: Vec<IpAddr>,

    /// The hostname of this collector instance.
    #[arg(long)]
    pub source_hostname: String,

    /// The number of pings to send per second.
    #[arg(long)]
    pub rate: u64,

    /// The address of the zzping-database server.
    #[arg(long, default_value = "https://127.0.0.1:7878")]
    pub database_addr: String,

    /// The authentication token to use when connecting to the database.
    #[arg(long, default_value = "my-secret-token")]
    pub auth_token: String,

    /// The maximum number of pings in flight at any given time.
    ///
    /// This acts as a backpressure mechanism. If the network is slow or pings are
    /// timing out, this limit prevents the collector from flooding the network with
    /// an ever-increasing number of outstanding packets. A low number is recommended
    /// to avoid causing a denial-of-service-like event on the target host.
    #[arg(long, default_value = "3")]
    pub max_in_flight: usize,
}
