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
mod float;
mod map;
use float::Float;
use map::Map;

use rmp::decode::ValueReadError;
use rmp::decode::{
    read_data_f32, read_data_f64, read_data_i16, read_data_i32, read_data_i64, read_data_i8,
    read_data_u16, read_data_u32, read_data_u64, read_data_u8, read_marker, ExtMeta,
};
use rmp::Marker;

use std::{collections::HashMap, io::Read};

#[derive(Ord, PartialOrd, Eq, Hash, PartialEq, Debug)]
pub enum Variant {
    String(String),
    Integer(i128),
    Bool(bool),
    Array(Vec<Variant>),
    Binary(Vec<u8>),
    Null(()),
    Float(Float),
    Map(Map),
    Extension((i8, Vec<u8>)), // i8 is the extension type - app defined
    Reserved,
}

impl Variant {
    // We can add convenience functions here!
    // i.e. to ease the craziness of match recursively.
    // pub fn as_f64
}

#[derive(Debug)]
pub enum Type {
    String,
    Integer,
    Bool,
    Array,
    Binary,
    Null,
    Float,
    Map,
    Extension,
    Reserved,
}

impl Type {
    pub fn from_marker(marker: Marker) -> Type {
        match marker {
            // String
            Marker::FixStr(_) => Type::String,
            Marker::Str8 => Type::String,
            Marker::Str16 => Type::String,
            Marker::Str32 => Type::String,
            // Number: i128 (to fit all values, including u64)
            Marker::FixPos(_) => Type::Integer,
            Marker::FixNeg(_) => Type::Integer,
            Marker::U8 => Type::Integer,
            Marker::U16 => Type::Integer,
            Marker::U32 => Type::Integer,
            Marker::U64 => Type::Integer,
            Marker::I8 => Type::Integer,
            Marker::I16 => Type::Integer,
            Marker::I32 => Type::Integer,
            Marker::I64 => Type::Integer,
            // Bool
            Marker::True => Type::Bool,
            Marker::False => Type::Bool,
            // Array: Vec<Variant>
            Marker::FixArray(_) => Type::Array,
            Marker::Array16 => Type::Array,
            Marker::Array32 => Type::Array,
            // Binary: Vec<u8>, len u32
            Marker::Bin8 => Type::Binary,
            Marker::Bin16 => Type::Binary,
            Marker::Bin32 => Type::Binary,
            // Null
            Marker::Null => Type::Null,
            // Floats: f64
            Marker::F32 => Type::Float,
            Marker::F64 => Type::Float,
            // Maps
            Marker::FixMap(_) => Type::Map,
            Marker::Map16 => Type::Map,
            Marker::Map32 => Type::Map,
            // Extensions (application-defined)
            Marker::FixExt1 => Type::Extension,
            Marker::FixExt2 => Type::Extension,
            Marker::FixExt4 => Type::Extension,
            Marker::FixExt8 => Type::Extension,
            Marker::FixExt16 => Type::Extension,
            Marker::Ext8 => Type::Extension,
            Marker::Ext16 => Type::Extension,
            Marker::Ext32 => Type::Extension,
            // Reserved
            // Marker::Reserved => Type::Reserved,
            _ => Type::Reserved, // To provide support for unknown/new values in rmp
        }
    }
}

pub fn read_any<R: Read>(rd: &mut R) -> Result<Variant, ValueReadError> {
    let marker = read_marker(rd)?;
    let mtype = Type::from_marker(marker);
    match mtype {
        Type::Float => read_float(rd, marker).map(Float::new).map(Variant::Float),
        Type::Integer => read_int(rd, marker).map(Variant::Integer),
        Type::Bool => read_bool(marker).map(Variant::Bool),
        Type::String => read_str(rd, marker).map(Variant::String),
        _ => Err(ValueReadError::TypeMismatch(Marker::Reserved)),
    }
}

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

pub fn read_bool(marker: Marker) -> Result<bool, ValueReadError> {
    match marker {
        Marker::True => Ok(true),
        Marker::False => Ok(false),
        marker => Err(ValueReadError::TypeMismatch(marker)),
    }
}

pub fn read_array<R: Read>(rd: &mut R, marker: Marker) -> Result<Vec<Variant>, ValueReadError> {
    let len = read_array_len(rd, marker)?;
    let mut ret: Vec<Variant> = vec![];
    for _ in 0..len {
        let value = read_any(rd)?;
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

pub fn read_bin_len<R: Read>(rd: &mut R) -> Result<u32, ValueReadError> {
    match read_marker(rd)? {
        Marker::Bin8 => Ok(read_data_u8(rd)? as u32),
        Marker::Bin16 => Ok(read_data_u16(rd)? as u32),
        Marker::Bin32 => Ok(read_data_u32(rd)?),
        marker => Err(ValueReadError::TypeMismatch(marker)),
    }
}

pub fn read_nil<R: Read>(rd: &mut R) -> Result<(), ValueReadError> {
    match read_marker(rd)? {
        Marker::Null => Ok(()),
        marker => Err(ValueReadError::TypeMismatch(marker)),
    }
}

pub fn read_map<R: Read>(
    rd: &mut R,
    marker: Marker,
) -> Result<HashMap<Variant, Variant>, ValueReadError> {
    let len = read_map_len(rd, marker)?;
    let mut ret: HashMap<Variant, Variant> = HashMap::new();
    for _ in 0..len {
        let key = read_any(rd)?;
        let value = read_any(rd)?;
        // TODO
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

pub fn read_float<R: Read>(rd: &mut R, marker: Marker) -> Result<f64, ValueReadError> {
    match marker {
        Marker::F32 => Ok(read_data_f32(rd)? as f64),
        Marker::F64 => Ok(read_data_f64(rd)?),
        marker => Err(ValueReadError::TypeMismatch(marker)),
    }
}

pub fn read_ext_meta<R: Read>(rd: &mut R) -> Result<ExtMeta, ValueReadError> {
    let size = match read_marker(rd)? {
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
