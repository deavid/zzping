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

pub mod float;
pub mod map;
pub mod variant;
pub mod vtype;

use variant::Variant;

use rmp::decode::ValueReadError;
use rmp::decode::{
    read_data_f32, read_data_f64, read_data_i16, read_data_i32, read_data_i64, read_data_i8,
    read_data_u16, read_data_u32, read_data_u64, read_data_u8, ExtMeta,
};
use rmp::Marker;

use std::{collections::HashMap, io::Read};

// ---- STRING ---
pub fn read_str<R: Read>(rd: &mut R, marker: Marker) -> Result<String, ValueReadError> {
    let (len, _bytesread) = read_str_len_with_nread(rd, marker)?;
    read_str_data2(rd, len)
}

pub fn read_str_data2<R: Read>(rd: &mut R, len: u64) -> Result<String, ValueReadError> {
    let mut handle = rd.take(len);
    let mut ret = String::new();
    handle
        .read_to_string(&mut ret)
        .map(|_| ret)
        .map_err(ValueReadError::InvalidDataRead)
}

pub fn read_str_len_with_nread<R>(
    rd: &mut R,
    marker: Marker,
) -> Result<(u64, usize), ValueReadError>
where
    R: Read,
{
    match marker {
        Marker::FixStr(size) => Ok((size as u64, 1)),
        Marker::Str8 => Ok((read_data_u8(rd)? as u64, 2)),
        Marker::Str16 => Ok((read_data_u16(rd)? as u64, 3)),
        Marker::Str32 => Ok((read_data_u32(rd)? as u64, 5)),
        marker => Err(ValueReadError::TypeMismatch(marker)),
    }
}

// ---- INTEGER ----
pub fn read_int<R: Read>(rd: &mut R, marker: Marker) -> Result<i128, ValueReadError> {
    match marker {
        Marker::FixPos(val) => Ok(val as i128),
        Marker::FixNeg(val) => Ok(val as i128),
        Marker::U8 => read_data_u8(rd).map(|x| x as i128),
        Marker::U16 => read_data_u16(rd).map(|x| x as i128),
        Marker::U32 => read_data_u32(rd).map(|x| x as i128),
        Marker::U64 => read_data_u64(rd).map(|x| x as i128),
        Marker::I8 => read_data_i8(rd).map(|x| x as i128),
        Marker::I16 => read_data_i16(rd).map(|x| x as i128),
        Marker::I32 => read_data_i32(rd).map(|x| x as i128),
        Marker::I64 => read_data_i64(rd).map(|x| x as i128),
        marker => Err(ValueReadError::TypeMismatch(marker)),
    }
}

// --- BOOL ----
pub fn read_bool(marker: Marker) -> Result<bool, ValueReadError> {
    match marker {
        Marker::True => Ok(true),
        Marker::False => Ok(false),
        marker => Err(ValueReadError::TypeMismatch(marker)),
    }
}

// ---- ARRAY ----
pub fn read_array<R: Read>(rd: &mut R, marker: Marker) -> Result<Vec<Variant>, ValueReadError> {
    let len = read_array_len(rd, marker)?;
    let mut ret: Vec<Variant> = vec![];
    for _ in 0..len {
        let value = Variant::read(rd)?;
        ret.push(value);
    }
    Ok(ret)
}

pub fn read_array_len<R: Read>(rd: &mut R, marker: Marker) -> Result<usize, ValueReadError> {
    match marker {
        Marker::FixArray(size) => Ok(size as usize),
        Marker::Array16 => Ok(read_data_u16(rd)? as usize),
        Marker::Array32 => Ok(read_data_u32(rd)? as usize),
        marker => Err(ValueReadError::TypeMismatch(marker)),
    }
}

// ---- BINARY ---
pub fn read_bin<R: Read>(rd: &mut R, marker: Marker) -> Result<Vec<u8>, ValueReadError> {
    let len = read_bin_len(rd, marker)?;
    let mut v: Vec<u8> = vec![0; len];
    rd.read_exact(v.as_mut_slice())
        .map_err(ValueReadError::InvalidDataRead)?;

    Ok(v)
}

pub fn read_bin_len<R: Read>(rd: &mut R, marker: Marker) -> Result<usize, ValueReadError> {
    match marker {
        Marker::Bin8 => Ok(read_data_u8(rd)? as usize),
        Marker::Bin16 => Ok(read_data_u16(rd)? as usize),
        Marker::Bin32 => Ok(read_data_u32(rd)? as usize),
        marker => Err(ValueReadError::TypeMismatch(marker)),
    }
}

// ----- NULL -----
pub fn read_nil(marker: Marker) -> Result<(), ValueReadError> {
    match marker {
        Marker::Null => Ok(()),
        marker => Err(ValueReadError::TypeMismatch(marker)),
    }
}

// ----- MAP -----
pub fn read_map<R: Read>(
    rd: &mut R,
    marker: Marker,
) -> Result<HashMap<Variant, Variant>, ValueReadError> {
    let len = read_map_len(rd, marker)?;
    let mut ret: HashMap<Variant, Variant> = HashMap::new();
    for _ in 0..len {
        let key = Variant::read(rd)?;
        let value = Variant::read(rd)?;
        ret.insert(key, value);
    }
    Ok(ret)
}

pub fn read_map_len<R: Read>(rd: &mut R, marker: Marker) -> Result<usize, ValueReadError> {
    match marker {
        Marker::FixMap(size) => Ok(size as usize),
        Marker::Map16 => Ok(read_data_u16(rd)? as usize),
        Marker::Map32 => Ok(read_data_u32(rd)? as usize),
        marker => Err(ValueReadError::TypeMismatch(marker)),
    }
}

// ----- FLOAT -----
pub fn read_float<R: Read>(rd: &mut R, marker: Marker) -> Result<f64, ValueReadError> {
    match marker {
        Marker::F32 => Ok(read_data_f32(rd)? as f64),
        Marker::F64 => Ok(read_data_f64(rd)?),
        marker => Err(ValueReadError::TypeMismatch(marker)),
    }
}

// ----- APP EXTENSIONS -----
pub fn read_ext<R: Read>(rd: &mut R, marker: Marker) -> Result<(i8, Vec<u8>), ValueReadError> {
    let extmeta = read_ext_meta(rd, marker)?;
    let mut v: Vec<u8> = vec![0; extmeta.size as usize];
    rd.read_exact(v.as_mut_slice())
        .map_err(ValueReadError::InvalidDataRead)?;

    Ok((extmeta.typeid, v))
}

pub fn read_ext_meta<R: Read>(rd: &mut R, marker: Marker) -> Result<ExtMeta, ValueReadError> {
    let size = match marker {
        Marker::FixExt1 => 1,
        Marker::FixExt2 => 2,
        Marker::FixExt4 => 4,
        Marker::FixExt8 => 8,
        Marker::FixExt16 => 16,
        Marker::Ext8 => read_data_u8(rd)? as u32,
        Marker::Ext16 => read_data_u16(rd)? as u32,
        Marker::Ext32 => read_data_u32(rd)?,
        marker => return Err(ValueReadError::TypeMismatch(marker)),
    };

    let ty = read_data_i8(rd)?;
    let meta = ExtMeta { typeid: ty, size };

    Ok(meta)
}
