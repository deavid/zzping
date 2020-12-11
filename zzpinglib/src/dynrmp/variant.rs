use std::io::Read;

use rmp::decode::{read_marker, ValueReadError};

use super::map::Map;
use super::{float::Float, vtype::VType};
use super::{
    read_array, read_bin, read_bool, read_ext, read_float, read_int, read_map, read_nil, read_str,
};

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
    pub fn as_str(&self) -> &str {
        match self {
            Self::String(v) => &v,
            _ => panic!("Variant of incorrect type"),
        }
    }
    pub fn as_int(&self) -> i128 {
        match self {
            Self::Integer(v) => *v,
            _ => panic!("Variant of incorrect type"),
        }
    }
    pub fn as_bool(&self) -> bool {
        match self {
            Self::Bool(v) => *v,
            _ => panic!("Variant of incorrect type"),
        }
    }
    pub fn as_slice(&self) -> &[Variant] {
        match self {
            Self::Array(v) => &v,
            _ => panic!("Variant of incorrect type"),
        }
    }
    pub fn as_bin(&self) -> &[u8] {
        match self {
            Self::Binary(v) => &v,
            _ => panic!("Variant of incorrect type"),
        }
    }
    // Nulls can also be seen as Option<T>
    pub fn as_option(&self) -> Option<&Self> {
        match self {
            Self::Null(()) => None,
            _ => Some(&self),
        }
    }

    pub fn as_null(&self) {
        match self {
            Self::Null(()) => (),
            _ => panic!("Variant of incorrect type"),
        }
    }

    pub fn read<R: Read>(rd: &mut R) -> Result<Self, ValueReadError> {
        let marker = read_marker(rd)?;
        let mtype = VType::from_marker(marker);
        match mtype {
            VType::Float => read_float(rd, marker).map(Float::new).map(Variant::Float),
            VType::Integer => read_int(rd, marker).map(Variant::Integer),
            VType::Bool => read_bool(marker).map(Variant::Bool),
            VType::String => read_str(rd, marker).map(Variant::String),
            VType::Null => read_nil(marker).map(Variant::Null),
            VType::Array => read_array(rd, marker).map(Variant::Array),
            VType::Map => read_map(rd, marker)
                .map(Map::from_hashmap)
                .map(Variant::Map),
            VType::Binary => read_bin(rd, marker).map(Variant::Binary),
            VType::Extension => read_ext(rd, marker).map(Variant::Extension),
            // VType::Reserved,
            _ => Err(ValueReadError::TypeMismatch(marker)),
        }
    }
}
