use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Serialize, Deserialize, Debug)]
pub struct ServerConfig {
    pub udp_listen_address: String,
    pub udp_client_address: String,
    pub ping_targets: Vec<String>,
}

impl ServerConfig {
    pub fn from_file(filepath: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let contents = fs::read_to_string(filepath)?;
        Ok(ron::de::from_str(&contents)?)
    }
}
