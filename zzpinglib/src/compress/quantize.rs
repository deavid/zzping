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

use bit_vec::BitVec;

use super::{Compress, CompressTo, Error};
use std::convert::TryInto;

#[derive(Debug)]
pub struct LogQuantizer {
    pub data: Vec<u64>,
    pub precision: f32,  // Ratio of maximum log deviation (0.01 => 1%)
    pub zero_point: f32, // Minimum value allowed (autodetected)
    pub max_value: u64,  // Maximum value encoded (for bit calculation)
    pub bits: u8,        // Number of bits required to serialize one value
}
/*
WARN: This library has a problem. It's neither capable to encode zero values or
negative values.
To allow for zero+negative we need:
  - min_significant_value : f32 , which value is actually encoded as non-zero
  - zero_value: Option<u64>, where zero is encoded. If it is at all.

Still, encoding positive or negative-only values would be problematic.

WARN: This library also does not encode NaN, Infinity, Sub-normal or any non-real number.
*/

impl LogQuantizer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn decompress_data(&self, srcdata: &[u64]) -> Result<Vec<f32>, Error> {
        let log_shift: f32 = self.precision.ln_1p();
        let lg_zero_point = self.zero_point.ln();
        let data: Vec<_> = srcdata
            .iter()
            .map(|x| *x as f32 * log_shift)
            .map(|x| x + lg_zero_point)
            .map(|x| x.exp())
            .collect();

        Ok(data)
    }
}

impl Default for LogQuantizer {
    fn default() -> Self {
        Self {
            data: vec![],
            precision: 0.02, // 0.001 => 0.1%
            zero_point: 0.0,
            max_value: 0,
            bits: 0,
        }
    }
}
impl Compress<f32> for LogQuantizer {
    fn setup(
        &mut self,
        _params: std::collections::HashMap<String, crate::dynrmp::variant::Variant>,
    ) -> Result<(), Error> {
        Err(Error::ToDo)
    }

    fn compress(&mut self, data: &[f32]) -> Result<(), Error> {
        let log_shift: f32 = self.precision.ln_1p();
        self.zero_point = data.iter().fold(f32::MAX, |a, b| -> f32 { a.min(*b) });
        let lg_zero_point = self.zero_point.ln();
        self.data = data
            .iter()
            .map(|x| x.ln() - lg_zero_point)
            .map(|x| x / log_shift)
            .map(|x| x.round() as u64)
            .collect();
        self.max_value = self.data.iter().max().copied().unwrap();
        let bits = (self.max_value as f32).log2();
        self.bits = bits.ceil() as u8;
        // dbg!(log_shift);
        dbg!(lg_zero_point);
        dbg!(self.max_value);
        dbg!(bits);
        // dbg!(self.bits);
        // println!("{:?}", self.data);
        Ok(())
    }

    fn serialize(&self) -> Result<Vec<u8>, Error> {
        Ok(self
            .serialize_metadata()?
            .into_iter()
            .chain(self.serialize_data()?.into_iter())
            .collect())
    }

    fn decompress(&self) -> Result<Vec<f32>, Error> {
        self.decompress_data(&self.data)
    }
    fn debug_name(&self) -> String {
        format!("LogQuantizer<p:{}>", self.precision)
    }

    fn serialize_metadata(&self) -> Result<Vec<u8>, Error> {
        let prec: [u8; 4] = self.precision.to_be_bytes();
        let zero: [u8; 4] = self.zero_point.to_be_bytes();
        Ok(prec.iter().chain(zero.iter()).copied().collect())
    }

    fn serialize_data(&self) -> Result<Vec<u8>, Error> {
        // u32 is enough for >10 years of data. That probably doesn't fit in a single file
        let size: u32 = self.data.len() as u32;
        let bits: usize = self.bits as usize;
        let total_bits: usize = bits * size as usize;
        let mut buffer = BitVec::with_capacity(total_bits);
        for v in self.data.iter().copied() {
            let b = v.to_be_bytes();
            let mut vb = BitVec::from_bytes(&b); // Most expensive
            vb = vb.split_off(64 - bits);
            buffer.append(&mut vb);
        }
        assert_eq!(total_bits, buffer.len());
        let u8buf: Vec<u8> = buffer.to_bytes();
        let data: Vec<u8> = size
            .to_be_bytes()
            .iter()
            .chain(self.bits.to_be_bytes().iter())
            .chain(u8buf.iter())
            .copied()
            .collect();
        Ok(data)
    }

    fn deserialize_metadata(&mut self, payload: &[u8]) -> Result<usize, Error> {
        let prec: [u8; 4] = payload[0..4].try_into().unwrap();
        let zero: [u8; 4] = payload[4..8].try_into().unwrap();
        self.precision = f32::from_be_bytes(prec);
        self.zero_point = f32::from_be_bytes(zero);
        Ok(8)
    }

    fn deserialize_data(&mut self, payload: &[u8]) -> Result<usize, Error> {
        let bsize: [u8; 4] = payload[0..4].try_into().unwrap();
        let bbits: [u8; 1] = payload[4..5].try_into().unwrap();
        let size = u32::from_be_bytes(bsize) as usize;
        let bits = u8::from_be_bytes(bbits) as usize;
        self.bits = bits as u8;
        let total_bits: usize = size * bits;
        let total_bytes: usize = (total_bits + 7) / 8;
        let final_bytes = total_bytes + 5; // 5 bytes from header.
        let databits = BitVec::from_bytes(&payload[5..final_bytes]);
        self.data = Vec::with_capacity(size);
        let mut datiter = databits.iter();
        for _ in 0..size {
            let mut valuebits = datiter.by_ref().take(bits).collect(); // <- 2nd most expensive
            let mut bv = BitVec::from_elem(64 - bits, false);
            bv.append(&mut valuebits);
            let vec = bv.to_bytes(); // <- Most expensive operation!
            let bytes: [u8; 8] = vec.try_into().unwrap();
            let value: u64 = u64::from_be_bytes(bytes);
            assert!(value < (1 << bits));
            self.data.push(value);
        }
        Ok(final_bytes)
    }

    fn deserialize(&mut self, payload: &[u8]) -> Result<usize, Error> {
        let bits1 = self.deserialize_metadata(payload)?;
        let bits2 = self.deserialize_data(&payload[bits1..])?;
        Ok(bits1 + bits2)
    }
}

impl CompressTo<f32, u64> for LogQuantizer {
    fn get_data(&self) -> Result<&[u64], Error> {
        Ok(&self.data)
    }

    fn decompress_from(&self, srcdata: &[u64]) -> Result<Vec<f32>, Error> {
        self.decompress_data(&srcdata)
    }
}

//// -*---

#[derive(Debug)]
pub struct LinearQuantizer {
    pub data: Vec<u64>,
    pub max_value: u64, // Maximum value encoded (for bit calculation)
    pub min_point: f32, // Minimum value allowed (autodetected)
    pub max_point: f32, // Maximum value allowed (autodetected)
    pub bits: u8,       // Number of bits required to serialize one value
}

impl LinearQuantizer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn decompress_data(&self, srcdata: &[u64]) -> Result<Vec<f32>, Error> {
        let wide = self.max_point - self.min_point;
        let maxval = self.max_value as f32;

        Ok(srcdata
            .iter()
            .map(|x| *x as f32 * wide / maxval)
            .map(|x| x + self.min_point)
            .collect())
    }
}

impl Default for LinearQuantizer {
    fn default() -> Self {
        Self {
            data: vec![],
            max_value: 6080,
            min_point: 0.0,
            max_point: 0.0,
            bits: 0,
        }
    }
}
impl Compress<f32> for LinearQuantizer {
    fn setup(
        &mut self,
        _params: std::collections::HashMap<String, crate::dynrmp::variant::Variant>,
    ) -> Result<(), Error> {
        Err(Error::ToDo)
    }

    fn compress(&mut self, data: &[f32]) -> Result<(), Error> {
        self.min_point = data.iter().fold(f32::MAX, |a, b| -> f32 { a.min(*b) });
        self.max_point = data.iter().fold(f32::MIN, |a, b| -> f32 { a.max(*b) });
        let wide = self.max_point - self.min_point;
        let maxval = self.max_value as f32;
        self.data = data
            .iter()
            .map(|x| x - self.min_point)
            .map(|x| x * maxval / wide)
            .map(|x| x.round() as u64)
            .collect();
        self.bits = maxval.log2().ceil() as u8;
        Ok(())
    }

    fn serialize(&self) -> Result<Vec<u8>, Error> {
        Err(Error::ToDo)
    }

    fn deserialize(&mut self, _payload: &[u8]) -> Result<usize, Error> {
        Err(Error::ToDo)
    }

    fn decompress(&self) -> Result<Vec<f32>, Error> {
        self.decompress_data(&self.data)
    }
    fn debug_name(&self) -> String {
        format!("LinearQuantizer<v:{}>", self.max_value)
    }

    fn serialize_metadata(&self) -> Result<Vec<u8>, Error> {
        todo!()
    }

    fn serialize_data(&self) -> Result<Vec<u8>, Error> {
        todo!()
    }

    fn deserialize_metadata(&mut self, _payload: &[u8]) -> Result<usize, Error> {
        todo!()
    }

    fn deserialize_data(&mut self, _payload: &[u8]) -> Result<usize, Error> {
        todo!()
    }
}

impl CompressTo<f32, u64> for LinearQuantizer {
    fn get_data(&self) -> Result<&[u64], Error> {
        Ok(&self.data)
    }

    fn decompress_from(&self, srcdata: &[u64]) -> Result<Vec<f32>, Error> {
        self.decompress_data(&srcdata)
    }
}
