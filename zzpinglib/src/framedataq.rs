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

use chrono::{DateTime, NaiveDateTime, Utc};
use dynrmp::variant::Variant;

use crate::{
    compress::quantize::LinearLogQuantizer,
    dynrmp,
    framedata::{FrameData, FrameTime},
};

#[derive(Debug, Clone, Copy)]
pub enum SubSecType {
    Abs(u32),
    Delta(u32),
}

impl SubSecType {
    pub fn unwrap_delta(&self) -> u32 {
        match self {
            SubSecType::Delta(v) => *v,
            SubSecType::Abs(_) => panic!("Expected a delta value, found an absolute one"),
        }
    }

    pub fn unwrap_abs(&self) -> u32 {
        match self {
            SubSecType::Abs(v) => *v,
            SubSecType::Delta(_) => panic!("Expected an absolute value, found a delta"),
        }
    }

    pub fn unwrap_abs_or_add(&self, reference: u32) -> u32 {
        match self {
            SubSecType::Abs(v) => *v,
            SubSecType::Delta(v) => *v + reference,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FrameDataQ {
    pub timestamp: Option<i64>,
    pub subsec_ms: SubSecType,
    pub inflight: usize,
    pub lost_packets: usize,
    pub recv_us_len: usize,
    pub recv_us: [i64; 7],
}

impl FrameDataQ {
    pub fn from_framedata(fd: &FrameData) -> Self {
        let mut tsv = match fd.time {
            FrameTime::Timestamp(t) => (Some(t), 0),
            FrameTime::Elapsed(e) => (None, e.as_millis() as u32),
        };
        let ts: Option<DateTime<Utc>> = tsv.0.take();
        let e = tsv.1 + ts.map(|x| x.timestamp_subsec_millis()).unwrap_or_default();

        Self {
            timestamp: ts.map(|x| x.timestamp()),
            subsec_ms: SubSecType::Abs(e),
            inflight: fd.inflight,
            lost_packets: fd.lost_packets,
            recv_us_len: fd.recv_us.len(),
            recv_us: Self::compute_percentiles(&fd.recv_us),
        }
    }
    pub fn get_datetime(&self) -> DateTime<Utc> {
        let ts = self.timestamp.unwrap();
        let subsec_ms = self.subsec_ms.unwrap_abs();
        let dt = NaiveDateTime::from_timestamp_opt(ts, subsec_ms).unwrap();
        DateTime::from_utc(dt, Utc)
    }
    pub fn compute_percentiles(v: &[u128]) -> [i64; 7] {
        let mut ret = [-1_i64; 7];
        if v.is_empty() {
            return ret;
        }
        let percentiles = [0f32, 0.125, 0.25, 0.5, 0.75, 0.875, 1.0];
        let vmax = v.len() - 1;
        for (i, p) in percentiles.iter().enumerate() {
            let p = *p * vmax as f32;
            let (pl, pr) = (p.floor() as usize, p.ceil() as usize);
            if pl == pr {
                ret[i] = v[pl] as i64;
            } else {
                let fr = p - pl as f32;
                let fl = 1.0 - fr;
                ret[i] = (v[pl] as f32 * fl + v[pr] as f32 * fr).round() as i64;
            }
        }
        ret
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FDCodecCfg {
    /// Amount of time between to fully encode the timestamp
    pub full_encode_secs: i64,
    /// Quantization encoding for recv_us
    pub recv_llq: Option<LinearLogQuantizer>,
}

impl Default for FDCodecCfg {
    fn default() -> Self {
        Self {
            full_encode_secs: 60,
            recv_llq: None,
        }
    }
}
#[derive(Debug, Clone, Copy)]
pub struct FDCodecState {
    cfg: FDCodecCfg,
    pub last_timestamp: Option<i64>,
    pub last_subsec_ms: u32,
}

impl FDCodecState {
    pub fn new(cfg: FDCodecCfg) -> Self {
        let mut s = Self::default();
        s.cfg = cfg;
        s
    }

    pub fn get_cfg(&self) -> FDCodecCfg {
        self.cfg
    }

    pub fn push(&mut self, d: &FrameDataQ) {
        if let Some(ts) = d.timestamp {
            self.last_timestamp = Some(ts);
        }
        match d.subsec_ms {
            SubSecType::Abs(v) => self.last_subsec_ms = v,
            SubSecType::Delta(v) => self.last_subsec_ms += v,
        };
    }
    pub fn peek_encode(&self, mut d: FrameDataQ) -> FrameDataQ {
        let mut d_ts = d.timestamp.unwrap();
        let subsec_ms = match d.subsec_ms {
            SubSecType::Abs(v) => v,
            SubSecType::Delta(v) => self.last_subsec_ms + v,
        };
        let subsec_ms_part = subsec_ms % 1000;
        d_ts += ((subsec_ms - subsec_ms_part) / 1000) as i64;

        let extra_subsecs: Option<u32> = match self.last_timestamp {
            Some(last_ts) => {
                if d_ts - last_ts >= self.cfg.full_encode_secs || d_ts < last_ts {
                    None
                } else {
                    Some(((d_ts - last_ts) * 1000) as u32)
                }
            }

            None => None,
        };
        match extra_subsecs {
            None => {
                d.timestamp = Some(d_ts);
                d.subsec_ms = SubSecType::Abs(subsec_ms_part);
            }
            Some(extra_subsecs) => {
                d.timestamp = None;
                d.subsec_ms =
                    SubSecType::Delta(extra_subsecs + subsec_ms_part - self.last_subsec_ms);
            }
        };
        if let Some(llq) = self.cfg.recv_llq {
            if d.recv_us_len > 0 {
                for val in d.recv_us.iter_mut() {
                    *val = llq.encode(*val);
                }
            }
        }
        d
    }

    pub fn encode(&mut self, d: FrameDataQ) -> FrameDataQ {
        let d = self.peek_encode(d);
        self.push(&d);
        d
    }

    pub fn peek_decode(&self, mut d: FrameDataQ) -> FrameDataQ {
        let last_ts = self
            .last_timestamp
            .expect("Tried to decode delta without reference timestamp");
        let mut ts = d.timestamp.unwrap_or(last_ts);
        let subsec_ms = d.subsec_ms.unwrap_abs_or_add(self.last_subsec_ms);
        let subsec_ms_part = subsec_ms % 1000;
        ts += ((subsec_ms - subsec_ms_part) / 1000) as i64;
        d.timestamp = Some(ts);
        d.subsec_ms = SubSecType::Abs(subsec_ms_part);
        if let Some(llq) = self.cfg.recv_llq {
            if d.recv_us_len > 0 {
                for val in d.recv_us.iter_mut() {
                    *val = llq.decode(*val);
                }
            }
        }

        d
    }

    pub fn decode(&mut self, d: FrameDataQ) -> FrameDataQ {
        let d = self.peek_decode(d);
        self.push(&d);
        d
    }
}

impl Default for FDCodecState {
    fn default() -> Self {
        Self {
            cfg: FDCodecCfg::default(),
            last_timestamp: None,
            last_subsec_ms: 0,
        }
    }
}

#[derive(Debug)]
pub enum Error {
    RmpEncodeValue(rmp::encode::ValueWriteError),
    RmpDecodeValue(rmp::decode::ValueReadError),
    RmpDecodeNumValue(rmp::decode::NumValueReadError),
    Variant(dynrmp::variant::Error),
    StdIO(std::io::Error),
    UnexpectedData(String),
}

impl Error {
    fn unexpected_data(s: &str) -> Self {
        Self::UnexpectedData(s.to_owned())
    }
}

impl From<dynrmp::variant::Error> for Error {
    fn from(e: dynrmp::variant::Error) -> Self {
        Self::Variant(e)
    }
}
impl From<rmp::encode::ValueWriteError> for Error {
    fn from(e: rmp::encode::ValueWriteError) -> Self {
        Self::RmpEncodeValue(e)
    }
}

impl From<rmp::decode::ValueReadError> for Error {
    fn from(e: rmp::decode::ValueReadError) -> Self {
        Self::RmpDecodeValue(e)
    }
}

impl From<rmp::decode::NumValueReadError> for Error {
    fn from(e: rmp::decode::NumValueReadError) -> Self {
        Self::RmpDecodeNumValue(e)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: rmp::encode::Error) -> Self {
        Self::StdIO(e)
    }
}

pub trait RMPCodec: Sized + std::fmt::Debug {
    fn try_to_rmp(&self) -> Result<Vec<u8>, Error>;
    fn try_from_rmp<R: std::io::Read>(rd: &mut R) -> Result<Self, Error>;

    fn to_rmp(&self) -> Vec<u8> {
        match self.try_to_rmp() {
            Err(e) => panic!(
                "Error trying to serialize to MsgPck: {:?}.\nValue:\n{:?}",
                e, self
            ),
            Ok(v) => v,
        }
    }
    fn from_rmp<R: std::io::Read>(rd: &mut R) -> Self {
        match Self::try_from_rmp(rd) {
            Err(e) => panic!("Error trying to deserialize from MsgPck: {:?}", e),
            Ok(v) => v,
        }
    }
}

impl RMPCodec for FrameDataQ {
    fn try_to_rmp(&self) -> Result<Vec<u8>, Error> {
        let mut data: Vec<u8> = vec![];
        let buf = &mut data;
        let subsec_ms;
        match self.timestamp {
            Some(val) => {
                rmp::encode::write_uint(buf, val as u64)?;
                subsec_ms = self.subsec_ms.unwrap_abs();
            }
            None => {
                rmp::encode::write_nil(buf)?;
                subsec_ms = self.subsec_ms.unwrap_delta();
            }
        }
        rmp::encode::write_uint(buf, subsec_ms as u64)?;
        rmp::encode::write_uint(buf, self.inflight as u64)?;
        rmp::encode::write_uint(buf, self.lost_packets as u64)?;
        rmp::encode::write_uint(buf, self.recv_us_len as u64)?;
        if self.recv_us_len > 0 {
            rmp::encode::write_array_len(buf, 7)?;
            // TODO: LinearLogQuantizer::encode
            let mut prev = 0;
            for v in &self.recv_us {
                let dv = *v - prev;
                prev = *v;
                rmp::encode::write_uint(buf, dv as u64)?;
            }
        }

        Ok(data)
    }

    fn try_from_rmp<R: std::io::Read>(rd: &mut R) -> Result<Self, Error> {
        let ts_var = Variant::read(rd)?;
        let timestamp = match ts_var {
            Variant::Null(_) => None,
            Variant::Integer(v) => Some(v as i64),
            _ => return Err(Error::unexpected_data("want Null or Int")),
        };
        let subsec_ms_v: usize = rmp::decode::read_int(rd)?;
        let subsec_ms = match timestamp {
            Some(_) => SubSecType::Abs(subsec_ms_v as u32),
            None => SubSecType::Delta(subsec_ms_v as u32),
        };
        let inflight: usize = rmp::decode::read_int(rd)?;
        let lost_packets: usize = rmp::decode::read_int(rd)?;
        let recv_us_len: usize = rmp::decode::read_int(rd)?;
        let mut recv_us: [i64; 7] = [-1, -1, -1, -1, -1, -1, -1];
        if recv_us_len > 0 {
            let recv_var_t = Variant::read(rd)?;
            let recv_var = recv_var_t.slice()?;
            // TODO: LinearLogQuantizer::decode
            let mut prev = 0;
            for (n, var) in recv_var.iter().enumerate() {
                let dv = var.int()?;
                let v = dv + prev;
                recv_us[n] = v as i64;
                prev = v;
            }
        }
        Ok(Self {
            timestamp,
            subsec_ms,
            inflight,
            lost_packets,
            recv_us_len,
            recv_us,
        })
    }
}
