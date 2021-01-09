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

use rmp::Marker;

#[derive(Debug, Clone, Copy)]
pub enum VType {
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

impl VType {
    pub fn from_marker(marker: Marker) -> VType {
        match marker {
            // String
            Marker::FixStr(_) => VType::String,
            Marker::Str8 => VType::String,
            Marker::Str16 => VType::String,
            Marker::Str32 => VType::String,
            // Number: i128 (to fit all values, including u64)
            Marker::FixPos(_) => VType::Integer,
            Marker::FixNeg(_) => VType::Integer,
            Marker::U8 => VType::Integer,
            Marker::U16 => VType::Integer,
            Marker::U32 => VType::Integer,
            Marker::U64 => VType::Integer,
            Marker::I8 => VType::Integer,
            Marker::I16 => VType::Integer,
            Marker::I32 => VType::Integer,
            Marker::I64 => VType::Integer,
            // Bool
            Marker::True => VType::Bool,
            Marker::False => VType::Bool,
            // Array: Vec<Variant>
            Marker::FixArray(_) => VType::Array,
            Marker::Array16 => VType::Array,
            Marker::Array32 => VType::Array,
            // Binary: Vec<u8>, len u32
            Marker::Bin8 => VType::Binary,
            Marker::Bin16 => VType::Binary,
            Marker::Bin32 => VType::Binary,
            // Null
            Marker::Null => VType::Null,
            // Floats: f64
            Marker::F32 => VType::Float,
            Marker::F64 => VType::Float,
            // Maps
            Marker::FixMap(_) => VType::Map,
            Marker::Map16 => VType::Map,
            Marker::Map32 => VType::Map,
            // Extensions (application-defined)
            Marker::FixExt1 => VType::Extension,
            Marker::FixExt2 => VType::Extension,
            Marker::FixExt4 => VType::Extension,
            Marker::FixExt8 => VType::Extension,
            Marker::FixExt16 => VType::Extension,
            Marker::Ext8 => VType::Extension,
            Marker::Ext16 => VType::Extension,
            Marker::Ext32 => VType::Extension,
            // Reserved
            // Marker::Reserved => VType::Reserved,
            _ => VType::Reserved, // To provide support for unknown/new values in rmp
        }
    }
}
