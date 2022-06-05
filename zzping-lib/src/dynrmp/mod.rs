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

pub mod float;
pub mod map;
pub mod variant;
pub mod vtype;

use variant::Variant;

use rmp::decode::{
    read_f32, read_f64, read_i16, read_i32, read_i64, read_i8, read_u16, read_u32, read_u64,
    read_u8, DecodeStringError, ExtMeta,
};
use rmp::decode::{MarkerReadError, ValueReadError};
use rmp::Marker;

use std::{collections::HashMap, io::Read};

use self::vtype::VType;
use anyhow::{Context, Result};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DError {
    #[error("I/O error: {0:?}")]
    IOError(std::io::Error),
    #[error("UnexpectedType: want: {0:?}, got {1:?}")]
    UnexpectedType(VType, VType), // want, got
    #[error("Marker read error: {0:?}")]
    RMPMarkerReadError(MarkerReadError),
    #[error("Value read error: {0:?}")]
    RMPValueReadError(ValueReadError),
    #[error("decode string error: {0:?}")]
    RMPDecodeStringError(String),
    #[error("traceback: {0:?}")]
    Traceback(String, Box<DError>),
}

impl DError {
    pub fn traceback(self, text: &str) -> Self {
        Self::Traceback(text.to_owned(), Box::new(self))
    }
    pub fn is_marker_eof(&self) -> bool {
        match self {
            DError::RMPMarkerReadError(de) => {
                matches!(de.0.kind(), std::io::ErrorKind::UnexpectedEof)
            }
            _ => false,
        }
    }
}

impl From<MarkerReadError> for DError {
    fn from(e: MarkerReadError) -> Self {
        Self::RMPMarkerReadError(e)
    }
}

impl From<ValueReadError> for DError {
    fn from(e: ValueReadError) -> Self {
        Self::RMPValueReadError(e)
    }
}

impl From<DecodeStringError<'_>> for DError {
    fn from(e: DecodeStringError) -> Self {
        Self::RMPDecodeStringError(e.to_string())
    }
}

// ---- STRING ---
pub fn read_str<R: Read>(rd: &mut R) -> Result<String, DError> {
    let mut buffer = [0_u8; 4096];
    Ok(rmp::decode::read_str(rd, &mut buffer)?.to_owned())
}

pub fn read_str_data2<R: Read>(rd: &mut R, len: u64) -> Result<String, ValueReadError> {
    let mut handle = rd.take(len);
    let mut ret = String::new();
    handle
        .read_to_string(&mut ret)
        .map(|_| ret)
        .map_err(ValueReadError::InvalidDataRead)
}

pub fn read_str_len_with_nread<R>(rd: &mut R, marker: Marker) -> Result<(u64, usize)>
where
    R: Read,
{
    match marker {
        Marker::FixStr(size) => Ok((size as u64, 1)),
        Marker::Str8 => Ok((read_u8(rd)? as u64, 2)),
        Marker::Str16 => Ok((read_u16(rd)? as u64, 3)),
        Marker::Str32 => Ok((read_u32(rd)? as u64, 5)),
        marker => {
            Err(ValueReadError::TypeMismatch(marker)).context("dynrmp::read_str_len_with_nread")
        }
    }
}

// ---- INTEGER ----
pub fn read_int<R: Read>(rd: &mut R, marker: Marker) -> Result<i128> {
    Ok(match marker {
        Marker::FixPos(val) => val as i128,
        Marker::FixNeg(val) => val as i128,
        Marker::U8 => read_u8(rd).map(|x| x as i128)?,
        Marker::U16 => read_u16(rd).map(|x| x as i128)?,
        Marker::U32 => read_u32(rd).map(|x| x as i128)?,
        Marker::U64 => read_u64(rd).map(|x| x as i128)?,
        Marker::I8 => read_i8(rd).map(|x| x as i128)?,
        Marker::I16 => read_i16(rd).map(|x| x as i128)?,
        Marker::I32 => read_i32(rd).map(|x| x as i128)?,
        Marker::I64 => read_i64(rd).map(|x| x as i128)?,
        marker => Err(ValueReadError::TypeMismatch(marker)).context("dynrmp::read_int")?,
    })
}

// --- BOOL ----
pub fn read_bool(marker: Marker) -> Result<bool> {
    Ok(match marker {
        Marker::True => true,
        Marker::False => false,
        marker => Err(ValueReadError::TypeMismatch(marker)).context("dynrmp::read_bool")?,
    })
}

// ---- ARRAY ----
pub fn read_array<R: Read>(rd: &mut R) -> Result<Vec<Variant>> {
    let len = rmp::decode::read_array_len(rd).context("dynrmp:read_array::len")?;
    let mut ret: Vec<Variant> = vec![];
    for _ in 0..len {
        let value = Variant::read(rd).context("dynrmp:read_array::var_read")?;
        ret.push(value);
    }
    Ok(ret)
}

pub fn read_array_len<R: Read>(rd: &mut R, marker: Marker) -> Result<usize> {
    match marker {
        Marker::FixArray(size) => Ok(size as usize),
        Marker::Array16 => Ok(read_u16(rd)? as usize),
        Marker::Array32 => Ok(read_u32(rd)? as usize),
        marker => Err(ValueReadError::TypeMismatch(marker)).context("read_array_len")?,
    }
}

// ---- BINARY ---
pub fn read_bin<R: Read>(rd: &mut R, marker: Marker) -> Result<Vec<u8>> {
    let len = read_bin_len(rd, marker).context("read_bin:len")?;
    let mut v: Vec<u8> = vec![0; len];
    rd.read_exact(v.as_mut_slice())
        .map_err(ValueReadError::InvalidDataRead)
        .context("read_bin:data")?;

    Ok(v)
}

pub fn read_bin_len<R: Read>(rd: &mut R, marker: Marker) -> Result<usize> {
    match marker {
        Marker::Bin8 => Ok(read_u8(rd)? as usize),
        Marker::Bin16 => Ok(read_u16(rd)? as usize),
        Marker::Bin32 => Ok(read_u32(rd)? as usize),
        marker => Err(ValueReadError::TypeMismatch(marker)).context("read_bin_len")?,
    }
}

// ----- NULL -----
pub fn read_nil(marker: Marker) -> Result<()> {
    match marker {
        Marker::Null => Ok(()),
        marker => Err(ValueReadError::TypeMismatch(marker)).context("read_nil")?,
    }
}

// ----- MAP -----
pub fn read_map<R: Read>(rd: &mut R, marker: Marker) -> Result<HashMap<Variant, Variant>> {
    let len = read_map_len(rd, marker).context("read_map - len")?;
    let mut ret: HashMap<Variant, Variant> = HashMap::new();
    for _ in 0..len {
        let key = Variant::read(rd).context("read_map::key")?;
        let value = Variant::read(rd).context("read_map::value")?;
        ret.insert(key, value);
    }
    Ok(ret)
}

pub fn read_map_len<R: Read>(rd: &mut R, _marker: Marker) -> Result<usize> {
    rmp::decode::read_map_len(rd)
        .map(|x| x as usize)
        .context("read_map_len")
    // let mut buf = [0_u8];
    // let _ = rd.read(&mut buf);
    // match marker {
    //     Marker::FixMap(size) => Ok(size as usize),
    //     Marker::Map16 => Ok(read_u16(rd)? as usize),
    //     Marker::Map32 => Ok(read_u32(rd)? as usize),
    //     marker => Err(ValueReadError::TypeMismatch(marker)),
    // }
}

// ----- FLOAT -----
pub fn read_float<R: Read>(rd: &mut R, marker: Marker) -> Result<f64> {
    match marker {
        Marker::F32 => Ok(read_f32(rd)? as f64),
        Marker::F64 => Ok(read_f64(rd)?),
        marker => Err(ValueReadError::TypeMismatch(marker)).context("read_float")?,
    }
}

// ----- APP EXTENSIONS -----
pub fn read_ext<R: Read>(rd: &mut R, marker: Marker) -> Result<(i8, Vec<u8>)> {
    let extmeta = read_ext_meta(rd, marker).context("read_ext:meta")?;
    let mut v: Vec<u8> = vec![0; extmeta.size as usize];
    rd.read_exact(v.as_mut_slice())
        .map_err(ValueReadError::InvalidDataRead)
        .context("read_ext:data")?;

    Ok((extmeta.typeid, v))
}

pub fn read_ext_meta<R: Read>(rd: &mut R, marker: Marker) -> Result<ExtMeta> {
    let size = match marker {
        Marker::FixExt1 => 1,
        Marker::FixExt2 => 2,
        Marker::FixExt4 => 4,
        Marker::FixExt8 => 8,
        Marker::FixExt16 => 16,
        Marker::Ext8 => read_u8(rd)? as u32,
        Marker::Ext16 => read_u16(rd)? as u32,
        Marker::Ext32 => read_u32(rd)?,
        marker => Err(ValueReadError::TypeMismatch(marker)).context("read_ext_meta:marker")?,
    };

    let ty = read_i8(rd)?;
    let meta = ExtMeta { typeid: ty, size };

    Ok(meta)
}
