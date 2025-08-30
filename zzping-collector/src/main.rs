use anyhow::Result;
use clap::Parser;
use log::{error, info};
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;

mod connection_manager;
mod pinger;

/// A high-frequency ICMP pinger that sends results to a zzping-database server.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// The IP address to ping
    #[arg(long)]
    target: IpAddr,

    /// The number of pings to send per second
    #[arg(long)]
    rate: u64,

    /// The address of the zzping-database server
    #[arg(long, default_value = "127.0.0.1:7878")]
    database_addr: String,

    /// The maximum number of pings in flight
    #[arg(long, default_value = "1000")]
    max_in_flight: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .init();
    let cli = Arc::new(Cli::parse());
    info!("Starting zzping-collector");
    info!("Target: {}", cli.target);
    info!("Rate: {} pps", cli.rate);
    info!("Database address: {}", cli.database_addr);
    info!("Max in-flight: {}", cli.max_in_flight);

    loop {
        info!("Attempting to connect to database at {}", cli.database_addr);
        match TcpStream::connect(&cli.database_addr).await {
            Ok(stream) => {
                info!("Successfully connected to database.");
                if let Err(e) = connection_manager::handle_connection(stream, cli.clone()).await {
                    error!("Error during connection handling: {e}. Reconnecting...");
                }
            }
            Err(e) => {
                error!("Failed to connect to database: {e}. Retrying in 5 seconds.");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use bytes::BufMut;
    use byteorder::{BigEndian, ReadBytesExt};
    use std::io::{Cursor, Read};
    use zzping_lib::protocol::RawDataRecord;

    #[test]
    fn test_record_serialization_and_deserialization() {
        // 1. Create a sample record
        let record = RawDataRecord {
            sent_nanos: 123456789,
            rtt_nanos: 987654321,
        };

        // 2. Serialize it to JSON
        let json_data = serde_json::to_vec(&record).unwrap();

        // 3. Create the length-prefixed packet
        let mut packet = Vec::new();
        packet.put_u32(json_data.len() as u32);
        packet.extend_from_slice(&json_data);

        // 4. Read the packet back to verify the format
        let mut cursor = Cursor::new(packet);

        // Read length prefix
        let len = cursor.read_u32::<BigEndian>().unwrap();
        assert_eq!(len as usize, json_data.len());

        // Read payload
        let mut buffer = vec![0; len as usize];
        cursor.read_exact(&mut buffer).unwrap();

        // 5. Deserialize and assert equality
        let received_record: RawDataRecord = serde_json::from_slice(&buffer).unwrap();
        assert_eq!(record, received_record);
    }
}
