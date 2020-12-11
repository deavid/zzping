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

use super::dynrmp::variant::Variant;
use chrono::{DateTime, Utc};
use std::time::Duration;

fn custom_error<E>(t: E) -> rmp::decode::ValueReadError
where
    E: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    rmp::decode::ValueReadError::InvalidDataRead(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        t,
    ))
}

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
    pub fn decode<R: std::io::Read>(rd: &mut R) -> Result<Self, rmp::decode::ValueReadError> {
        let t = Variant::read(rd)?;
        let time: FrameTime = match t {
            Variant::String(s) => {
                let elapsed = rmp::decode::read_u32(rd)?;
                if elapsed != 0 {
                    return Err(custom_error("Unexpected elapsed time, should be zero."));
                }
                FrameTime::Timestamp(
                    DateTime::parse_from_rfc3339(&s)
                        .map_err(custom_error)?
                        .with_timezone(&Utc),
                )
            }
            Variant::Integer(i) => FrameTime::Elapsed(Duration::from_micros(i as u64)),
            _ => panic!("Unexpected type"),
        };
        let inflight = rmp::decode::read_u16(rd)? as usize;
        let lost_packets = rmp::decode::read_u16(rd)? as usize;
        let recv_us_len = rmp::decode::read_array_len(rd)? as usize;
        let mut recv_us: Vec<u128> = Vec::with_capacity(recv_us_len);
        for _ in 0..recv_us_len {
            recv_us.push(rmp::decode::read_u32(rd)? as u128);
        }
        Ok(Self {
            time,
            inflight,
            lost_packets,
            recv_us,
        })
    }
}
