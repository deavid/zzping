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

//! Former format for storing logs from zzping-daemon. It's being phased out.

use crate::dynrmp;
use crate::dynrmp::variant::Variant;

use chrono::{DateTime, Utc};
use rmp::decode::ValueReadError;
use std::time::Duration;

use anyhow::{Context, Result};

/// Used to easily bundle errors from dynrmp
fn custom_error<E>(t: E) -> dynrmp::DError
where
    E: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    dynrmp::DError::RMPValueReadError(rmp::decode::ValueReadError::InvalidDataRead(
        std::io::Error::new(std::io::ErrorKind::InvalidData, t),
    ))
}

/// Timestamp part of FrameData
#[derive(Debug, Clone)]
pub enum FrameTime {
    /// On full frame encoding, a complete datetime is stored.
    Timestamp(DateTime<Utc>),
    // On regular delta-encoding, a duration since the last timestamp is stored.
    Elapsed(Duration),
}

#[derive(Debug, Clone)]
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
    pub fn decode<R: std::io::Read>(rd: &mut R) -> Result<Self> {
        let t = Variant::read(rd)?;
        let time: FrameTime = match t {
            Variant::String(s) => {
                let elapsed = rmp::decode::read_u32(rd)?;
                if elapsed != 0 {
                    return Err(custom_error("Unexpected elapsed time, should be zero."))?;
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

#[derive(Debug, Default)]
pub struct FrameDataVec {
    pub last_keyframe: Option<DateTime<Utc>>,
    pub v: Vec<FrameData>,
}

impl FrameDataVec {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn read<R: std::io::Read>(&mut self, rd: &mut R, count: u64) -> Result<()> {
        let err = || ValueReadError::TypeMismatch(rmp::Marker::Str8);
        for n in 0..count {
            println!("{}", n);
            let mut fd = FrameData::decode(rd)?;
            match &fd.time {
                FrameTime::Timestamp(ts) => self.last_keyframe = Some(*ts),
                FrameTime::Elapsed(e) => {
                    fd.time = FrameTime::Timestamp(
                        self.last_keyframe
                            .ok_or_else(err)
                            .context("FrameDataVec::read - no last_keyframe")?
                            + chrono::Duration::from_std(*e).unwrap(),
                    )
                }
            }
            self.v.push(fd);
        }
        Ok(())
    }
}
