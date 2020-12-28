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

use std::collections::HashMap;

// use bit_vec::BitVec;

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum ValueType {
    Raw,
    Corrected,
}

#[derive(Debug)]
pub struct HuffmanKey {
    pub qtype: ValueType,
    pub key: i64,
    pub extra_bits: usize,
    pub extra_data: i64,
    pub metadata: HKeyMetadata,
}

#[derive(Debug)]
pub struct HuffmanPartialKey {
    pub qtype: ValueType,
    pub key: i64,
    pub extra_bits: usize,
    pub metadata: HKeyMetadata,
}

impl HuffmanPartialKey {
    pub fn add_extra_data(&self, extra_data: i64) -> HuffmanKey {
        HuffmanKey {
            qtype: self.qtype,
            key: self.key,
            extra_bits: self.extra_bits,
            extra_data,
            metadata: HKeyMetadata::None,
        }
    }
}

pub struct Sech2Fn {
    precision: f64,
    item_count: usize, // This is just for others to estimate / get on the same order of magnitude
    function: Vec<f64>,
}

impl Sech2Fn {
    pub fn sech(v: f64) -> f64 {
        2.0 / (v.exp() + (-v).exp())
    }
    pub fn sech2(v: f64) -> f64 {
        Self::sech(v).powi(2)
    }

    pub fn new(precision: f64, item_count: usize) -> Self {
        Self {
            precision,
            item_count,
            function: vec![],
        }
    }
    pub fn compute_fn(&mut self, size: usize) {
        self.function = Vec::with_capacity(size);
        let stdev = self.precision.recip().powf(1.0 / 1.6);
        let items: f64 = self.item_count as f64;
        for i in 0..size {
            let v1: f64 = Self::sech2(i as f64 / stdev) * items;
            let v2: f64 = Self::sech2(i as f64 / stdev.powf(1.5)) * items.cbrt();
            let k: f64 = if i == 0 { 2.0 } else { 1.0 };
            self.function.push((v1 + v2) * k);
        }
    }
    pub fn get_fn(&self) -> Vec<f64> {
        self.function.clone()
    }
    pub fn get_range(&self, from: usize, to: usize) -> u64 {
        self.function[from..to].iter().sum::<f64>().ceil() as u64
    }
}

// Now we need to compose a HashMap from the above function.
// This hashmap is:
// * 0     special, as its frequency is doubled from the original.
// * 1:1   from -15   to 15.     -- 31 items                  (src: |  0 - 15|)
// * 1:4   from |16|  to |79|    -- 32 items (2 extra bits)   (src: | 16 - 31|)
// * 1:16  from |80|  to |335|   -- 32 items (4 extra bits)   (src: | 32 - 47|)
// * 1:256 from |336| to |4432|  -- 32 items (8 extra bits)   (src: | 48 - 63|)
//
// For 0.1% precision:
//  - 50% is expected to land under 65
//  - 80% under 162
//  - 90% under 425
//  - 95% under 624
//
// Then for the other, raw, not-diff values, we need a few tokens:
// * A: 16 bit value
// Extra tokens for higher precisions:
// * B: 32 bit value
// * C: 64 bit value
//
//
// The raw tokens will be worth frequency 1, while the others will be the sum
// of the frequency values of the functions. Therefore 'item_count' represents
// how many items you expect to encode between raw values.

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HKeyMetadata {
    Block(HuffmanMapSBlock),
    Raw(HuffmanMapSRaw),
    None,
}

impl Default for HKeyMetadata {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HuffmanMapSBlock {
    start_quantized: i64,
    start_huffman: i64,
    end_quantized: i64,
    end_huffman: i64,
    blocks: i64,
    block_size: i64,
    block_size_bits: i64,
}

impl HuffmanMapSBlock {
    pub fn tr_corrected2hk(&self, vq: i64) -> HuffmanKey {
        assert!(vq >= self.start_quantized && vq < self.end_quantized);
        let v = vq - self.start_quantized;
        let vb = v / self.block_size;
        let extra = v % self.block_size;
        let huffkey = vb + self.start_huffman;
        HuffmanKey {
            qtype: ValueType::Corrected,
            key: huffkey,
            extra_bits: self.block_size_bits as usize,
            extra_data: extra,
            metadata: HKeyMetadata::Block(*self),
        }
    }
    pub fn tr_hk2corrected(&self, hk: HuffmanKey) -> i64 {
        assert!(hk.qtype == ValueType::Corrected);
        assert!(hk.key >= self.start_huffman && hk.key < self.end_huffman);
        let vb = hk.key - self.start_huffman;
        let v = vb * self.block_size + hk.extra_data;
        v + self.start_quantized
    }

    pub fn tr_raw2hk(&self, _raw: i64) -> HuffmanKey {
        panic!("HuffmanBlocks cannot encode Raw values!")
    }
    pub fn tr_hk2raw(&self, _hk: HuffmanKey) -> i64 {
        panic!("HuffmanBlocks cannot decode Raw values!")
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HuffmanMapSRaw {
    /// How to name the token for raw input
    key: i64,
    /// How many bits will contain
    bits: usize,
    /// How frequent it will be considered compared to f
    freq: u64,
}

impl HuffmanMapSRaw {
    pub fn new(raw_key: i64, raw_bits: usize, raw_freq: u64) -> Self {
        Self {
            key: raw_key,
            bits: raw_bits,
            freq: raw_freq,
        }
    }
}

#[derive(Default)]
pub struct HuffmanMapS {
    f: Option<Sech2Fn>,
    map: HashMap<i64, u64>,
    raw_start: i64,
    raw: Vec<HuffmanMapSRaw>,
    blocks: Vec<HuffmanMapSBlock>,
}

impl HuffmanMapS {
    pub fn update_from_fn(&mut self) {
        // Maybe it doesn't make sense to support other stuff. 12 bit raw only.
        self.raw_start = 1_000_000;
        self.raw = vec![HuffmanMapSRaw::new(1_000_012, 12, 64)];
        self.blocks = vec![];
        self.map = HashMap::with_capacity(256);
        let mut cur = (0, 0);
        cur = self.update_from_fn_range(cur, 1, 0);
        cur = self.update_from_fn_range(cur, 16, 1);
        cur = self.update_from_fn_range(cur, 16, 2);
        cur = self.update_from_fn_range(cur, 16, 4);
        cur = self.update_from_fn_range(cur, 16, 8);
        dbg!(cur);
        for r in self.raw.iter() {
            self.map.insert(r.key, r.freq);
        }
        let mut mapv: Vec<_> = self.map.iter().collect();
        mapv.sort_unstable();
        for (k, v) in mapv.iter() {
            println!("{}:\t{}", k, v);
        }
    }
    pub fn update_from_fn_range(
        &mut self,
        start: (i64, i64),
        blocks: i64,
        bsize_bits: i64,
    ) -> (i64, i64) {
        let f = self.f.as_ref().unwrap();
        let bsize = 1 << bsize_bits;
        dbg!((start, blocks, bsize));
        for bnum in 0..blocks {
            let from = start.0 + bnum * bsize;
            let to = from + bsize;
            let k: i64 = start.1 + bnum;
            let v = f.get_range(from as usize, to as usize);
            self.map.insert(k, v);
            if k > 0 {
                self.map.insert(-k, v);
            }
        }
        let end_quantized = start.0 + blocks * bsize;
        let end_huffman = start.1 + blocks;

        let blck = HuffmanMapSBlock {
            start_quantized: start.0,
            start_huffman: start.1,
            end_quantized,
            end_huffman,
            blocks,
            block_size: bsize,
            block_size_bits: bsize_bits,
        };
        self.blocks.push(blck);
        (end_quantized, end_huffman)
    }

    pub fn get_partial_hkey(&self, hkey: i64) -> HuffmanPartialKey {
        let qtype = if hkey < self.raw_start {
            ValueType::Corrected
        } else {
            ValueType::Raw
        };
        let extra_bits = 0;
        let metadata = HKeyMetadata::None;

        HuffmanPartialKey {
            qtype,
            key: hkey,
            extra_bits,
            metadata,
        }
    }
    /// Given a raw or corrected value, output the hashmap key + extra bits.
    pub fn to_hkey(&self, qtype: ValueType, value: i64) -> HuffmanKey {
        match qtype {
            ValueType::Raw => HuffmanKey {
                qtype: ValueType::Raw,
                key: self.raw[0].key,
                extra_bits: self.raw[0].bits,
                extra_data: value,
                metadata: HKeyMetadata::Raw(self.raw[0]),
            },
            ValueType::Corrected => {
                let hsblock = self.get_qval_block(value).unwrap();
                hsblock.tr_corrected2hk(value)
            }
        }
    }

    pub fn get_qval_block(&self, qval: i64) -> Option<HuffmanMapSBlock> {
        let qval = qval.abs();
        for blck in self.blocks.iter() {
            if blck.start_quantized >= qval && blck.end_quantized < qval {
                return Some(*blck);
            }
        }
        None
    }
    pub fn get_hkey_block(&self, hkey: i64) -> Option<HuffmanMapSBlock> {
        let hkey = hkey.abs();
        for blck in self.blocks.iter() {
            if blck.start_huffman >= hkey && blck.end_huffman < hkey {
                return Some(*blck);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::HuffmanMapS;
    use super::Sech2Fn;

    #[test]
    fn test1() {
        let mut f = Sech2Fn::new(0.001, 1000000);
        f.compute_fn(16000);
        let mut m = HuffmanMapS::default();
        m.f = Some(f);
        m.update_from_fn();
    }
}
