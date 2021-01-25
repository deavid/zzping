// Copyright 2021 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
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

use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Default, Serialize, Deserialize, Debug, Clone)]
pub struct GuiConfig {
    pub udp_listen_address: String,
    pub udp_server_address: String,
    pub display_address: Vec<String>,
    pub sample_limit: usize,
}

impl GuiConfig {
    pub fn from_filepath(filepath: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let contents = fs::read_to_string(filepath)?;
        Ok(ron::de::from_str(&contents)?)
    }
}

#[derive(Default, Debug, Clone)]
pub struct OtherOpts {
    pub input_file: Option<String>,
}

#[derive(Default, Debug, Clone)]
pub struct Flags {
    pub guiconfig: GuiConfig,
    pub otheropts: OtherOpts,
}
