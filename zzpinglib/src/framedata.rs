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

use chrono::{DateTime, Utc};
use std::time::Duration;

pub enum FrameTime {
    Timestamp(DateTime<Utc>),
    Elapsed(Duration),
}
pub struct FrameData {
    pub time: FrameTime,
    pub inflight: usize,
    pub lost_packets: usize,
    pub recv_us: Vec<u128>,
}

impl FrameData {
    pub fn encode<W: std::io::Write>(
        &self,
        wr: &mut W,
    ) -> Result<(), rmp::encode::ValueWriteError> {
        let mut v: Vec<u8> = vec![];
        match self.time {
            FrameTime::Timestamp(now) => {
                let strnow = now.to_rfc3339_opts(chrono::SecondsFormat::Micros, false);
                rmp::encode::write_str(&mut v, &strnow)?;
                rmp::encode::write_u32(&mut v, 0)?;
            }
            FrameTime::Elapsed(elapsed) => {
                rmp::encode::write_u32(&mut v, elapsed.as_micros() as u32)?;
            }
        }
        rmp::encode::write_u16(&mut v, self.inflight as u16)?;
        rmp::encode::write_u16(&mut v, self.lost_packets as u16)?;

        rmp::encode::write_array_len(&mut v, self.recv_us.len() as u32)?;
        for val in self.recv_us.iter() {
            rmp::encode::write_u32(&mut v, *val as u32)?;
        }
        wr.write(&v)
            .map_err(rmp::encode::ValueWriteError::InvalidDataWrite)?;
        Ok(())
    }
    pub fn decode<R: std::io::Read>(_rd: &mut R) /*-> Self*/
    {
        // let marker = rmp::decode::read_marker(rd).unwrap();

        /*rmp::decode::read_str(rd, buf)
        match marker {
            rmp::Marker::FixStr(s) => (),
        }*/
    }
}
