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

use std::io::Read;

use rmp::decode::{read_marker, ValueReadError};
use rmp::Marker;

use super::map::Map;
use super::{float::Float, vtype::VType};
use super::{
    read_array, read_bin, read_bool, read_ext, read_float, read_int, read_map, read_nil, read_str,
    DError,
};

use anyhow::{Context, Result};

#[derive(Ord, PartialOrd, Eq, Hash, PartialEq, Debug, Clone)]
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
    pub fn get_type(&self) -> VType {
        match self {
            Variant::String(_) => VType::String,
            Variant::Integer(_) => VType::Integer,
            Variant::Bool(_) => VType::Bool,
            Variant::Array(_) => VType::Array,
            Variant::Binary(_) => VType::Binary,
            Variant::Null(_) => VType::Null,
            Variant::Float(_) => VType::Float,
            Variant::Map(_) => VType::Map,
            Variant::Extension(_) => VType::Extension,
            Variant::Reserved => VType::Reserved,
        }
    }
    fn err_unexpected<T>(&self, want: VType) -> Result<T> {
        Err(DError::UnexpectedType(want, self.get_type()))?
    }
    pub fn map(&self) -> Result<Map> {
        match self {
            Self::Map(v) => Ok(v.clone()),
            _ => self.err_unexpected(VType::Map),
        }
    }
    pub fn as_str(&self) -> &str {
        self.str().unwrap()
    }
    pub fn str(&self) -> Result<&str> {
        match self {
            Self::String(v) => Ok(v),
            _ => self.err_unexpected(VType::String),
        }
    }
    pub fn string(&self) -> Result<String> {
        match self {
            Self::String(v) => Ok(v.to_string()),
            _ => self.err_unexpected(VType::String),
        }
    }
    pub fn as_int(&self) -> i128 {
        match self {
            Self::Integer(v) => *v,
            _ => panic!("Variant of incorrect type"),
        }
    }
    pub fn int(&self) -> Result<i128> {
        match self {
            Self::Integer(v) => Ok(*v),
            _ => self.err_unexpected(VType::Integer),
        }
    }
    pub fn as_bool(&self) -> bool {
        match self {
            Self::Bool(v) => *v,
            _ => panic!("Variant of incorrect type"),
        }
    }
    pub fn as_slice(&self) -> &[Variant] {
        self.slice().unwrap()
    }
    pub fn slice(&self) -> Result<&[Variant]> {
        match self {
            Self::Array(v) => Ok(v),
            _ => self.err_unexpected(VType::Array),
        }
    }
    pub fn as_bin(&self) -> &[u8] {
        match self {
            Self::Binary(v) => v,
            _ => panic!("Variant of incorrect type"),
        }
    }
    // Nulls can also be seen as Option<T>
    pub fn as_option(&self) -> Option<&Self> {
        match self {
            Self::Null(()) => None,
            _ => Some(self),
        }
    }

    pub fn as_null(&self) {
        match self {
            Self::Null(()) => (),
            _ => panic!("Variant of incorrect type"),
        }
    }

    pub fn read_marker<R: Read>(rd: &mut R) -> Result<rmp::Marker, DError> {
        Ok(read_marker(rd)?)
    }

    fn read_from_marker<R: Read>(rd: &mut R, marker: rmp::Marker) -> Result<Self> {
        let mtype = VType::from_marker(marker);
        Ok(match mtype {
            VType::Float => read_float(rd, marker).map(Float::new).map(Variant::Float)?,
            VType::Integer => read_int(rd, marker).map(Variant::Integer)?,
            VType::Bool => read_bool(marker).map(Variant::Bool)?,
            VType::String => read_str(rd).map(Variant::String)?,
            VType::Null => read_nil(marker).map(Variant::Null)?,
            VType::Array => read_array(rd).map(Variant::Array)?,
            VType::Map => read_map(rd, marker)
                .map(Map::from_hashmap)
                .map(Variant::Map)?,
            VType::Binary => read_bin(rd, marker).map(Variant::Binary)?,
            VType::Extension => read_ext(rd, marker).map(Variant::Extension)?,
            // VType::Reserved,
            _ => {
                return Err(DError::RMPValueReadError(ValueReadError::TypeMismatch(
                    marker,
                )))
                .context("read_from_marker")
            }
        })
    }

    pub fn read<R: Read>(rd: &mut R) -> Result<Self> {
        let mut mk_byte = [0_u8];
        rd.read_exact(&mut mk_byte).map_err(DError::IOError)?;
        let marker = Marker::from_u8(mk_byte[0]);

        let mut pfixreader = PrefixRead {
            prefix: Some(mk_byte[0]),
            reader: rd,
        };
        Self::read_from_marker(&mut pfixreader, marker).context("read::read_from_marker")
    }
}

struct PrefixRead<'a> {
    prefix: Option<u8>,
    reader: &'a mut dyn Read,
}

impl std::io::Read for PrefixRead<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self.prefix.take() {
            Some(pfix) => {
                buf[0] = pfix;
                Ok(1)
            }
            None => self.reader.read(buf),
        }
    }
}
