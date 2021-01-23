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

use super::custom_errors::UnexpectedError;

pub struct UdpStats {
    pub addr: String,
    pub inflight_count: u16,
    pub avg_time_us: u32,
    pub last_pckt_ms: u32,
    pub packet_loss_x100_000: u32,
}

impl UdpStats {
    pub fn from_buf(mut v: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        let addr: String;
        let inflight_count: u16;
        let avg_time_us: u32;
        let last_pckt_ms: u32;
        let packet_loss_x100_000: u32;

        let len = rmp::decode::read_array_len(&mut v)?;
        if len != 5 {
            return Err(Box::new(UnexpectedError::new("Array must be length 5")));
        }
        let mut buf: Vec<u8> = vec![0; 65536];
        addr = rmp::decode::read_str(&mut v, &mut buf)
            .map_err(|_| Box::new(UnexpectedError::new("Couldn't read string")))?
            .to_owned();

        inflight_count = rmp::decode::read_u16(&mut v)?;
        avg_time_us = rmp::decode::read_u32(&mut v)?;
        last_pckt_ms = rmp::decode::read_u32(&mut v)?;
        packet_loss_x100_000 = rmp::decode::read_u32(&mut v)?;

        Ok(Self {
            addr,
            inflight_count,
            avg_time_us,
            last_pckt_ms,
            packet_loss_x100_000,
        })
    }
}
