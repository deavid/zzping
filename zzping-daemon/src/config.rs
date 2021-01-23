// Copyright 2021 Google LLC
//
// Licen inflight_secs: (), lost_secs: (), recv_secs: ()sed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Reads the ping daemon configuration from a file.
//!
//! # Examples
//!
//! daemon_config.ron:
//! ```ron
//! ServerConfig(
//!     udp_listen_address: "127.0.0.1:7878",
//!     udp_client_address: "127.0.0.1:7879",
//!     ping_targets: [
//!         "192.168.0.1",
//!     ],
//! )
//! ```

use serde::{Deserialize, Serialize};
use std::fs;

/// Config for a single target host
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TargetHost {
    /// Target Host to ping, IP Address in string format
    pub address: String,
    /// How many pings per second to do
    pub frequency: u32,
}

impl TargetHost {
    #[allow(dead_code)]
    pub fn new(address: &str, frequency: u32) -> Self {
        Self {
            address: address.to_owned(),
            frequency,
        }
    }
}

/// Config for how long to keep the old pings
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ForgetConfig {
    pub inflight_secs: u64,
    pub lost_secs: u64,
    pub recv_secs: u64,
}

/// Configuration parameters for the pinger daemon
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ServerConfig {
    /// IP Address:port where the GUI would connect to.
    pub udp_listen_address: String,
    /// IP Address:port where the GUI is listening.
    pub udp_client_address: String,
    /// List of hosts that will be pinged.
    pub ping_targets: Vec<TargetHost>,
    /// How long to keep the packets
    pub keep_packets: ForgetConfig,
    /// Precision multiplier. Use 1.0 for low CPU usage, but frequencies might get off. 10.0 for excellent precision.
    pub precision_mult: f64,
}

impl ServerConfig {
    /// Reads a file located in 'filepath' and constructs a ServerConfig from it.
    pub fn from_filepath(filepath: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let contents = fs::read_to_string(filepath)?;
        Self::from_str(&contents)
    }
    /// Constructs a ServerConfig from the string passed.
    pub fn from_str(contents: &str) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(ron::de::from_str(contents)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const SAMPLE_CFG: &str = r#"
        ServerConfig(
            udp_listen_address: "127.0.0.1:7878",
            udp_client_address: "127.0.0.1:7879",
            ping_targets: [
                TargetHost(
                    address: "192.168.0.1",
                    frequency: 10,
                ),
            ],
            keep_packets: (
                inflight_secs: 10,
                lost_secs: 10,
                recv_secs: 10,
            ),
            precision_mult: 1.0,
        )        
    "#;

    #[test]
    fn test_from_str_empty() {
        let config = "";
        if let Ok(_cfg) = ServerConfig::from_str(&config) {
            panic!("This should have returned an error");
        }
    }
    #[test]
    fn test_from_str_valid() {
        match ServerConfig::from_str(&SAMPLE_CFG) {
            Err(e) => {
                dbg!(e);
                panic!("Error constructing the config");
            }
            Ok(cfg) => {
                assert_eq!(cfg.udp_listen_address, "127.0.0.1:7878");
                assert_eq!(cfg.udp_client_address, "127.0.0.1:7879");
                assert_eq!(cfg.ping_targets, vec![TargetHost::new("192.168.0.1", 10)]);
                assert!((cfg.precision_mult - 1.0).abs() < f64::EPSILON);
                assert_eq!(
                    cfg.keep_packets,
                    ForgetConfig {
                        inflight_secs: 10,
                        lost_secs: 10,
                        recv_secs: 10,
                    }
                );
            }
        }
    }
    #[test]
    fn test_from_file_valid() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();

        write!(tmpfile.as_file_mut(), "{}", &SAMPLE_CFG).unwrap();
        // Close the file, but keep the path to it around.
        let path = tmpfile.into_temp_path();
        dbg!(&path);
        match ServerConfig::from_filepath(path.to_str().unwrap()) {
            Err(e) => {
                dbg!(e);
                panic!("Error constructing the config");
            }
            Ok(cfg) => {
                assert_eq!(cfg.udp_listen_address, "127.0.0.1:7878");
                assert_eq!(cfg.udp_client_address, "127.0.0.1:7879");
                assert_eq!(cfg.ping_targets, vec![TargetHost::new("192.168.0.1", 10)]);
                assert!((cfg.precision_mult - 1.0).abs() < f64::EPSILON);
                assert_eq!(
                    cfg.keep_packets,
                    ForgetConfig {
                        inflight_secs: 10,
                        lost_secs: 10,
                        recv_secs: 10,
                    }
                );
            }
        }
        path.close().unwrap();
    }
    #[test]
    fn test_from_file_nofile() {
        match ServerConfig::from_filepath("") {
            Err(e) => {
                dbg!(e);
            }
            Ok(_cfg) => {
                panic!("This should have failed, filepath is empty");
            }
        }
    }
}
