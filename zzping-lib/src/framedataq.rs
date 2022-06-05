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

use std::{collections::VecDeque, marker::PhantomData};

use chrono::{DateTime, NaiveDateTime, Utc};
use dynrmp::variant::Variant;

use anyhow::{Context, Result};
use thiserror::Error;

#[allow(unused_imports)]
use crate::dbgf;

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
pub struct Complete;
#[derive(Debug, Clone, Copy)]
pub struct Encoded;

#[derive(Debug, Clone, Copy)]
pub struct FrameDataQ<T> {
    pub phantom: PhantomData<T>,
    pub timestamp: Option<i64>,
    pub subsec_ms: SubSecType,
    pub inflight: f32,
    pub lost_packets: f32,
    pub recv_us_len: usize,
    pub recv_us: [i64; 7],
}

impl<Complete> std::fmt::Display for FrameDataQ<Complete> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!(
            "FrameDataQ<Complete> {} i:{} l:{} sz:{}\t{:?}",
            self.get_datetime(),
            self.inflight,
            self.lost_packets,
            self.recv_us_len,
            self.recv_us,
        ))
    }
}

impl<Complete> FrameDataQ<Complete> {
    pub fn from_framedata(fd: &FrameData) -> Self {
        let mut tsv = match fd.time {
            FrameTime::Timestamp(t) => (Some(t), 0),
            FrameTime::Elapsed(e) => (None, e.as_millis() as u32),
        };
        let ts: Option<DateTime<Utc>> = tsv.0.take();
        let e = tsv.1 + ts.map(|x| x.timestamp_subsec_millis()).unwrap_or_default();

        Self {
            phantom: PhantomData::default(),
            timestamp: ts.map(|x| x.timestamp()),
            subsec_ms: SubSecType::Abs(e),
            inflight: fd.inflight as f32,
            lost_packets: fd.lost_packets as f32,
            recv_us_len: fd.recv_us.len(),
            recv_us: Self::compute_percentiles(&fd.recv_us),
        }
    }
    pub fn get_datetime(&self) -> DateTime<Utc> {
        let ts = self.timestamp.unwrap();
        let subsec_ms = self.subsec_ms.unwrap_abs();
        let dt = NaiveDateTime::from_timestamp_opt(ts, subsec_ms * 1000 * 1000).unwrap();
        DateTime::from_utc(dt, Utc)
    }
    pub fn get_timestamp_ms(&self) -> i128 {
        let ts = self.timestamp.unwrap();
        let subsec_ms = self.subsec_ms.unwrap_abs();
        ts as i128 * 1000 + subsec_ms as i128
    }
    // CAUTION:: This function requires the data to be pre-sorted, and all negative values removed!
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
            let val = if pl == pr {
                v[pl] as i64
            } else {
                let fr = p - pl as f32;
                let fl = 1.0 - fr;
                (v[pl] as f32 * fl + v[pr] as f32 * fr).round() as i64
            };
            ret[i] = val;
        }
        ret
    }
    fn into_encoded(self) -> FrameDataQ<Encoded> {
        FrameDataQ {
            phantom: PhantomData::default(),
            timestamp: self.timestamp,
            subsec_ms: self.subsec_ms,
            inflight: self.inflight,
            lost_packets: self.lost_packets,
            recv_us_len: self.recv_us_len,
            recv_us: self.recv_us,
        }
    }
    pub fn fold_vec(data: &[Self]) -> Self {
        let datalen = data.len();
        let vtimestamp_ms = data.iter().map(|x| x.get_timestamp_ms());
        let mean_ts: i128 = vtimestamp_ms.sum::<i128>() / datalen as i128;

        let timestamp = Some((mean_ts / 1000) as i64);
        let subsec_ms = SubSecType::Abs((mean_ts % 1000) as u32);
        // let inflight: usize = data.iter().map(|x| x.inflight).max().unwrap();
        let inflight: f32 = (data.iter().map(|x| x.inflight.powi(2)).sum::<f32>() as f32
            / datalen as f32)
            .powf(2.0_f32.recip());
        let lost_packets: f32 = data.iter().map(|x| x.lost_packets).sum::<f32>() / datalen as f32;
        let recv_us_len: usize = data.iter().map(|x| x.recv_us_len).sum::<usize>() / datalen;
        // TODO: This algorithm is wrong, we need to split each percentile and compute over it!
        let mut recv_us_list: Vec<u128> = data
            .iter()
            .flat_map(|x| x.recv_us.iter())
            .filter(|x| **x >= 0)
            .map(|x| *x as u128)
            .collect();
        recv_us_list.sort_unstable();
        let recv_us = Self::compute_percentiles(&recv_us_list);
        // let recv_v: Vec<_> = (0..7)
        //     .map(|n| {
        //         data.iter()
        //             .map(|x| x.recv_us[n])
        //             .filter(|x| *x > 0)
        //             .map(|x| x as u128)
        //             .collect::<Vec<_>>()
        //     })
        //     .collect();

        // let recv_us: [i64; 7] = [
        //     Self::compute_percentiles(&recv_v[0])[0],
        //     Self::compute_percentiles(&recv_v[1])[1],
        //     Self::compute_percentiles(&recv_v[2])[2],
        //     Self::compute_percentiles(&recv_v[3])[3],
        //     Self::compute_percentiles(&recv_v[4])[4],
        //     Self::compute_percentiles(&recv_v[5])[5],
        //     Self::compute_percentiles(&recv_v[6])[6],
        // ];

        FrameDataQ {
            phantom: PhantomData::default(),
            timestamp,
            subsec_ms,
            inflight,
            lost_packets,
            recv_us_len,
            recv_us,
        }
    }
}

impl<Encoded> FrameDataQ<Encoded> {
    fn into_complete(self) -> FrameDataQ<Complete> {
        FrameDataQ {
            phantom: PhantomData::default(),
            timestamp: self.timestamp,
            subsec_ms: self.subsec_ms,
            inflight: self.inflight,
            lost_packets: self.lost_packets,
            recv_us_len: self.recv_us_len,
            recv_us: self.recv_us,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FDCodecCfg {
    /// Amount of time between to fully encode the timestamp
    pub full_encode_secs: i64,
    /// Quantization encoding for recv_us
    pub recv_llq: Option<LinearLogQuantizer>,
    /// Enable delta encoding
    pub delta_enc: bool,
}

impl Default for FDCodecCfg {
    fn default() -> Self {
        Self {
            full_encode_secs: 60,
            recv_llq: None,
            delta_enc: false,
        }
    }
}
#[derive(Debug, Clone, Copy, Default)]
pub struct FDCodecState {
    cfg: FDCodecCfg,
    pub last_timestamp: Option<i64>,
    pub last_subsec_ms: u32,
    pub last_recvq_0: i64,
}

impl FDCodecState {
    const HEADER_SCHEMA: &'static str = "FDCodec";
    const HEADER_VERSION: u64 = 101;

    pub fn new(cfg: FDCodecCfg) -> Self {
        Self {
            cfg,
            ..Default::default()
        }
    }
    pub fn get_header(cfg: FDCodecCfg) -> Vec<u8> {
        Self::try_get_header(cfg).unwrap()
    }
    pub fn try_get_header(cfg: FDCodecCfg) -> Result<Vec<u8>, XError> {
        let mut vbuf: Vec<u8> = vec![];
        let wr = &mut vbuf;
        rmp::encode::write_map_len(wr, 5)?;
        rmp::encode::write_str(wr, "schema")?;
        rmp::encode::write_str(wr, Self::HEADER_SCHEMA)?;

        rmp::encode::write_str(wr, "version")?;
        rmp::encode::write_uint(wr, Self::HEADER_VERSION)?;

        rmp::encode::write_str(wr, "full_encode_secs")?;
        rmp::encode::write_uint(wr, cfg.full_encode_secs as u64)?;

        rmp::encode::write_str(wr, "recv_llq")?;
        match cfg.recv_llq {
            Some(llq) => rmp::encode::write_f64(wr, llq.get_precision())?,
            None => rmp::encode::write_nil(wr)?,
        }

        rmp::encode::write_str(wr, "delta_enc")?;
        rmp::encode::write_bool(wr, cfg.delta_enc)?;

        Ok(vbuf)
    }
    pub fn try_from_header<R: std::io::Read>(rd: &mut R) -> Result<FDCodecCfg> {
        let header = Variant::read(rd)?.map()?.into_strhashmap()?;
        let get_header = |field: &str| -> Result<&Variant, XError> {
            header
                .get(field)
                .ok_or_else(|| XError::header_field_missing(field))
        };
        let schema = get_header("schema")?.str()?;
        if schema != Self::HEADER_SCHEMA {
            return Err(XError::unexpected_data(
                "Incompatible header, wrong file format",
            ))?;
        }
        let version = get_header("version")?.int()? as u64;
        if version > Self::HEADER_VERSION {
            return Err(XError::unexpected_data(
                "File format has a newer, unsupported version",
            ))?;
        }
        let full_encode_secs = get_header("full_encode_secs")?.int()? as i64;
        let recv_llq = get_header("recv_llq")?;
        let delta_enc = get_header("delta_enc")?.as_bool();
        // Extra parameters should have a default to allow for processing older formats!

        let recv_llq = match recv_llq {
            Variant::Null(_) => Ok(None),
            Variant::Float(v) => Ok(Some(LinearLogQuantizer::new(v.as_f64()))),
            _ => Err(XError::unexpected_data(
                "recv_llq expected to be nil or float type",
            )),
        }?;
        Ok(FDCodecCfg {
            full_encode_secs,
            recv_llq,
            delta_enc,
        })
    }
    pub fn from_header<R: std::io::Read>(rd: &mut R) -> FDCodecCfg {
        Self::try_from_header(rd).unwrap()
    }

    pub fn new_from_header<R: std::io::Read>(rd: &mut R) -> Self {
        Self::new(Self::from_header(rd))
    }

    pub fn get_cfg(&self) -> FDCodecCfg {
        self.cfg
    }

    pub fn push(&mut self, d: &FrameDataQ<Complete>) {
        if let Some(ts) = d.timestamp {
            self.last_timestamp = Some(ts);
        }
        match d.subsec_ms {
            SubSecType::Abs(v) => self.last_subsec_ms = v,
            SubSecType::Delta(v) => self.last_subsec_ms += v,
        };
        if self.cfg.delta_enc {
            self.last_recvq_0 = match self.cfg.recv_llq {
                Some(llq) => llq.encode(d.recv_us[0]),
                None => d.recv_us[0],
            }
        }
    }
    pub fn peek_encode(&self, mut d: FrameDataQ<Complete>) -> FrameDataQ<Encoded> {
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
                    *val = llq.encode(*val) - self.last_recvq_0;
                }
            }
        }
        d.into_encoded()
    }

    pub fn encode(&mut self, d: FrameDataQ<Complete>) -> FrameDataQ<Encoded> {
        let dr = self.peek_encode(d);
        self.push(&d);
        dr
    }

    pub fn peek_decode(&self, mut d: FrameDataQ<Encoded>) -> FrameDataQ<Complete> {
        let mut ts = d.timestamp.unwrap_or_else(|| {
            self.last_timestamp
                .expect("Tried to decode delta without reference timestamp")
        });
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

        d.into_complete()
    }

    pub fn decode(&mut self, d: FrameDataQ<Encoded>) -> FrameDataQ<Complete> {
        let d = self.peek_decode(d);
        self.push(&d);
        d
    }
}

#[derive(Error, Debug)]
pub enum XError {
    #[error("msgpack write error: {0:?}")]
    RmpEncodeValue(rmp::encode::ValueWriteError),
    #[error("msgpack read error: {0:?}")]
    RmpDecodeValue(rmp::decode::ValueReadError),
    #[error("msgpack num value read error: {0:?}")]
    RmpDecodeNumValue(rmp::decode::NumValueReadError),
    #[error("dynrmp variant error: {0:?}")]
    Variant(dynrmp::DError),
    #[error("I/O error: {0:?}")]
    StdIO(std::io::Error),
    #[error("unexpected data: {0:?}")]
    UnexpectedData(String),
    #[error("header field missing: {0:?}")]
    HeaderFieldMissing(String),
    #[error("End of File")]
    EOF,
}

impl XError {
    fn unexpected_data(s: &str) -> Self {
        Self::UnexpectedData(s.to_owned())
    }
    fn header_field_missing(s: &str) -> Self {
        Self::HeaderFieldMissing(s.to_owned())
    }
}

impl From<dynrmp::DError> for XError {
    fn from(e: dynrmp::DError) -> Self {
        Self::Variant(e)
    }
}
impl From<rmp::encode::ValueWriteError> for XError {
    fn from(e: rmp::encode::ValueWriteError) -> Self {
        Self::RmpEncodeValue(e)
    }
}

impl From<rmp::decode::ValueReadError> for XError {
    fn from(e: rmp::decode::ValueReadError) -> Self {
        Self::RmpDecodeValue(e)
    }
}

impl From<rmp::decode::NumValueReadError> for XError {
    fn from(e: rmp::decode::NumValueReadError) -> Self {
        Self::RmpDecodeNumValue(e)
    }
}

impl From<std::io::Error> for XError {
    fn from(e: std::io::Error) -> Self {
        Self::StdIO(e)
    }
}

pub trait RMPCodec: Sized + std::fmt::Debug {
    fn try_to_rmp(&self) -> Result<Vec<u8>>;
    fn try_from_rmp<R: std::io::Read>(rd: &mut R) -> Result<Self>;

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

impl RMPCodec for FrameDataQ<Encoded> {
    fn try_to_rmp(&self) -> Result<Vec<u8>> {
        let mut data: Vec<u8> = vec![];
        let buf = &mut data;
        let subsec_ms = match self.timestamp {
            Some(val) => {
                rmp::encode::write_uint(buf, val as u64)?;
                self.subsec_ms.unwrap_abs()
            }
            None => {
                rmp::encode::write_nil(buf)?;
                self.subsec_ms.unwrap_delta()
            }
        };
        rmp::encode::write_uint(buf, subsec_ms as u64)?;
        if self.inflight.round() as usize + self.lost_packets.round() as usize > 0 {
            rmp::encode::write_uint(buf, self.inflight.round() as u64)?;
            rmp::encode::write_uint(buf, self.lost_packets.round() as u64)?;
        } else {
            rmp::encode::write_nfix(buf, -1)?;
        }
        rmp::encode::write_uint(buf, self.recv_us_len as u64)?;
        if self.recv_us_len > 0 {
            rmp::encode::write_array_len(buf, 7)?;
            let mut prev = 0;
            let mut dvvec = vec![];
            for v in &self.recv_us {
                let dv = *v - prev;
                // assert!(dv >= 0, "prev {} bigger than new {}", prev, v);
                prev = *v;
                dvvec.push(dv);
            }
            // dbgf!(&dvvec);
            // dbg!(format!("{:?}", &dvvec));
            for dv in dvvec {
                if dv < 0 {
                    rmp::encode::write_sint(buf, dv)?;
                } else {
                    rmp::encode::write_uint(buf, dv as u64)?;
                }
            }
        }

        Ok(data)
    }

    fn try_from_rmp<R: std::io::Read>(rd: &mut R) -> Result<Self> {
        let ts_var = Variant::read(rd)
            .map_err(|e| {
                let de = e.downcast_ref::<dynrmp::DError>();
                match de {
                    Some(dynrmp::DError::IOError(x)) => match x.kind() {
                        std::io::ErrorKind::UnexpectedEof => anyhow::Error::from(XError::EOF),
                        _ => e,
                    },
                    _ => e,
                }
            })
            .context("ts_var")?;
        let timestamp = match ts_var {
            Variant::Null(_) => None,
            Variant::Integer(v) => Some(v as i64),
            _ => Err(XError::unexpected_data("want Null or Int"))?,
        };
        let subsec_ms_v: usize = rmp::decode::read_int(rd).context("subsec_ms")?;
        let subsec_ms = match timestamp {
            Some(_) => SubSecType::Abs(subsec_ms_v as u32),
            None => SubSecType::Delta(subsec_ms_v as u32),
        };
        let ifl: i64 = rmp::decode::read_int(rd).context("inflight")?;
        let inflight: f32 = if ifl == -1 { 0.0 } else { ifl as f32 };
        let lost_packets: f32 = if ifl == -1 {
            0.0
        } else {
            rmp::decode::read_int::<usize, _>(rd).context("lost packets")? as f32
        };
        let recv_us_len: usize = rmp::decode::read_int(rd).context("recv_us_len")?;
        let mut recv_us: [i64; 7] = [-1, -1, -1, -1, -1, -1, -1];
        if recv_us_len > 0 {
            let recv_var_t = Variant::read(rd).context("recv_var_t")?;
            let recv_var = recv_var_t.slice().context("recv_var slice")?;
            // TODO: LinearLogQuantizer::decode
            let mut prev = 0;
            for (n, var) in recv_var.iter().enumerate() {
                let dv = var.int().context("recv_var.int")?;
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
            phantom: PhantomData::default(),
        })
    }
}

pub struct FDCodecIter<R: std::io::Read> {
    buf: R,
    fdcs: FDCodecState,
}

impl<R: std::io::Read> FDCodecIter<R> {
    pub fn new(mut buf: R) -> Self {
        let fdcs = FDCodecState::new_from_header(&mut buf);
        Self { buf, fdcs }
    }
}

impl<R: std::io::Read> Iterator for FDCodecIter<R> {
    type Item = FrameDataQ<Complete>;

    fn next(&mut self) -> Option<Self::Item> {
        let rfde = RMPCodec::try_from_rmp(&mut self.buf).context("FDCodecIter - next");
        match rfde {
            Ok(v) => Some(self.fdcs.decode(v)),
            Err(e) => {
                if matches!(e.downcast_ref::<XError>(), Some(XError::EOF)) {
                    None
                } else {
                    println!("FDCodecIter::iterator::next(): Error: {}", e);
                    for cause in e.chain() {
                        dbg!(cause);
                    }

                    panic!("Unexpected error ocurred while reading MessagePack from buffer");
                }
            }
        }
    }
}

pub struct FDCodecIterFold<T>
where
    T: Iterator<Item = FrameDataQ<Complete>>,
{
    iter: T,
    window: usize,
    step: usize,
    buffer: VecDeque<FrameDataQ<Complete>>, // ring buffer
}

impl<T> FDCodecIterFold<T>
where
    T: Iterator<Item = FrameDataQ<Complete>>,
{
    pub fn from_iter(iter: T, window: usize, step: usize) -> Self {
        assert!(window >= step);
        Self {
            window,
            step,
            iter,
            buffer: VecDeque::with_capacity(window),
        }
    }
}
impl<T> Iterator for FDCodecIterFold<T>
where
    T: Iterator<Item = FrameDataQ<Complete>>,
{
    type Item = FrameDataQ<Complete>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.window == 1 && self.step == 1 {
            self.iter.next()
        } else {
            // FIXME: This just does "nothing"
            let iter = &mut self.iter;
            let new_items: Vec<_> = iter.take(self.step).collect();
            let to_remove = match new_items.is_empty() {
                true => self.step as i64,
                false => (self.buffer.len() + self.step) as i64 - self.window as i64,
            };
            for _ in 0..to_remove {
                self.buffer.pop_back();
            }
            for value in new_items.into_iter() {
                self.buffer.push_front(value);
            }
            self.buffer.make_contiguous();
            if self.buffer.is_empty() {
                None
            } else {
                Some(FrameDataQ::fold_vec(self.buffer.as_slices().0))
            }
        }
    }
}

pub trait IterFold {
    type Iter;
    fn iter_fold(self, window: usize, step: usize) -> Self::Iter;
}

impl<T> IterFold for T
where
    T: Iterator<Item = FrameDataQ<Complete>>,
{
    type Iter = FDCodecIterFold<T>;
    fn iter_fold(self, window: usize, step: usize) -> Self::Iter {
        FDCodecIterFold::from_iter(self, window, step)
    }
}
