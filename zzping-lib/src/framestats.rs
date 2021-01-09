// Copyright 2020 Google LLC
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

use std::time::Duration;

pub struct FrameStats {
    pub addr_str: String,
    pub inflight_count: usize,
    pub avg_time_us: u128,
    pub last_pckt_ms: u128,
    pub packet_loss_x100_000: u32,
}

impl FrameStats {
    pub fn encode<W: std::io::Write>(
        &self,
        wr: &mut W,
    ) -> Result<(), rmp::encode::ValueWriteError> {
        rmp::encode::write_array_len(wr, 5)?;
        rmp::encode::write_str(wr, &self.addr_str)?;
        rmp::encode::write_u16(wr, self.inflight_count as u16)?;
        rmp::encode::write_u32(wr, self.avg_time_us as u32)?;
        rmp::encode::write_u32(wr, self.last_pckt_ms as u32)?;
        rmp::encode::write_u32(wr, self.packet_loss_x100_000)?;
        Ok(())
    }

    pub fn encode_stats(
        addr: std::net::IpAddr,
        inflight_count: usize,
        avg_time: Duration,
        last_pckt_received: Duration,
        packet_loss: f32,
    ) -> Result<Vec<u8>, String> {
        let mut v: Vec<u8> = vec![];
        let stat = Self {
            addr_str: addr.to_string(),
            inflight_count,
            avg_time_us: avg_time.as_micros(),
            last_pckt_ms: last_pckt_received.as_millis(),
            packet_loss_x100_000: (packet_loss * 1000.0) as u32,
        };
        match stat.encode(&mut v) {
            Ok(()) => Ok(v),
            Err(e) => Err(format!("encode_stats: FrameStats: {:?}", e)),
        }
    }
}
