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

use anyhow::{Context, Result};

use crate::custom_errors::GuiError;

pub struct UdpStats {
    pub addr: String,
    #[allow(dead_code)]
    pub inflight_count: u16,
    pub avg_time_us: u32,
    #[allow(dead_code)]
    pub last_pckt_ms: u32,
    pub packet_loss_x100_000: u32,
}

impl UdpStats {
    pub fn from_buf(mut v: &[u8]) -> Result<Self> {
        let len = rmp::decode::read_array_len(&mut v).context("UDPStats: len")?;

        if len != 5 {
            Err(GuiError::UnexpectedError(
                "UDPStats: Array must be length 5".into(),
            ))?;
        }
        let mut buf: Vec<u8> = vec![0; 65536];
        let addr = rmp::decode::read_str(&mut v, &mut buf)
            .map_err(|e| {
                GuiError::UnexpectedError(format!("UDPStats: Couldn't read string: {:?}", e))
            })?
            .to_owned();

        let inflight_count = rmp::decode::read_u16(&mut v).context("UDPStats: inflight_count")?;
        let avg_time_us = rmp::decode::read_u32(&mut v).context("UDPStats: avg_time_us")?;
        let last_pckt_ms = rmp::decode::read_u32(&mut v).context("UDPStats: last_pckt_ms")?;
        let packet_loss_x100_000 =
            rmp::decode::read_u32(&mut v).context("UDPStats: packet_loss_x100_000")?;

        Ok(Self {
            addr,
            inflight_count,
            avg_time_us,
            last_pckt_ms,
            packet_loss_x100_000,
        })
    }
}
