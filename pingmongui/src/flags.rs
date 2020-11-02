use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Default, Serialize, Deserialize, Debug)]
pub struct GuiConfig {
    pub udp_listen_address: String,
    pub udp_server_address: String,
}

impl GuiConfig {
    pub fn from_file(filepath: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let contents = fs::read_to_string(filepath)?;
        Ok(ron::de::from_str(&contents)?)
    }
}
