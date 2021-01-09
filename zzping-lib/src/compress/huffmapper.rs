use bit_vec::BitVec;

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
use super::corrector::DiffValue;
use super::corrector::ValueType;
use super::weightfn::WeightFn;
use std::collections::HashMap;

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
pub struct HuffmanMapSBlock {
    start_quantized: i64,
    start_huffman: i64,
    end_quantized: i64,
    end_huffman: i64,
    blocks: i64,
    block_size: i64,
    block_size_bits: usize,
}

impl HuffmanMapSBlock {
    pub fn tr_corrected2hk(&self, vq: i64) -> HuffmanKey {
        let sign = vq.signum();
        let vq = vq.abs();
        assert!(
            vq >= self.start_quantized && vq < self.end_quantized,
            format!("{} [{}..{}]", &vq, self.start_quantized, self.end_quantized)
        );
        let v = vq - self.start_quantized;
        let vb = v / self.block_size;
        let extra = v % self.block_size;
        let huffkey = vb + self.start_huffman;
        HuffmanKey {
            qtype: ValueType::Corrected,
            key: huffkey * sign,
            extra_bits: self.block_size_bits as usize,
            extra_data: extra,
            metadata: HKeyMetadata::Block(*self),
        }
    }
    pub fn tr_hk2corrected(&self, hk: HuffmanKey) -> i64 {
        // TODO: Negative keys are not handled yet!
        let sign = hk.key.signum();
        let key = hk.key.abs();
        assert!(hk.qtype == ValueType::Corrected);
        assert!(key >= self.start_huffman && key < self.end_huffman);
        let vb = key - self.start_huffman;
        let v = vb * self.block_size + hk.extra_data;
        (v + self.start_quantized) * sign
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

#[derive(Debug)]
pub struct HuffmanKey {
    pub qtype: ValueType,
    pub key: i64,
    pub extra_bits: usize,
    pub extra_data: i64,
    pub metadata: HKeyMetadata,
}

impl HuffmanKey {
    pub fn encode_extra(&self) -> BitVec {
        if self.extra_bits == 0 {
            return BitVec::new();
        }
        let extra: [u8; 8] = self.extra_data.to_be_bytes();
        let mut extravec = BitVec::from_bytes(&extra);
        let left_bit = 64 - self.extra_bits;
        extravec.split_off(left_bit)
    }
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

#[derive(Default)]
pub struct HuffmanMapS<T: WeightFn> {
    f: Option<T>,
    map: HashMap<i64, u64>,
    raw_start: i64,
    raw: Vec<HuffmanMapSRaw>,
    blocks: Vec<HuffmanMapSBlock>,
    symmetric: bool,
}

impl<T: WeightFn> HuffmanMapS<T> {
    pub fn new(f: T) -> Self {
        let mut m = Self {
            f: Some(f),
            map: Default::default(),
            raw_start: Default::default(),
            raw: Default::default(),
            blocks: Default::default(),
            symmetric: true,
        };
        m.update_from_fn();
        m
    }
    pub fn new_unsigned(f: T) -> Self {
        let mut m = Self {
            f: Some(f),
            map: Default::default(),
            raw_start: Default::default(),
            raw: Default::default(),
            blocks: Default::default(),
            symmetric: false,
        };
        m.update_from_fn();
        m
    }
    pub fn update_from_fn(&mut self) {
        let f = self.f.as_mut().unwrap();
        // TODO: We should guess which size do we need.
        f.compute_fn(256000);

        // Maybe it doesn't make sense to support other stuff. 12 bit raw only.
        self.raw_start = 1_000_000;
        self.raw = vec![HuffmanMapSRaw::new(1_000_012, 12, 32)];
        self.blocks = vec![];
        self.map = HashMap::with_capacity(256);
        let mut cur = (0, 0);
        cur = self.update_from_fn_range(cur, 1, 0);
        cur = self.update_from_fn_range(cur, 64, 0);
        cur = self.update_from_fn_range(cur, 64, 1);
        cur = self.update_from_fn_range(cur, 64, 2);
        cur = self.update_from_fn_range(cur, 128, 3);
        cur = self.update_from_fn_range(cur, 128, 4);
        cur = self.update_from_fn_range(cur, 128, 5);

        dbg!(cur);
        for r in self.raw.iter() {
            self.map.insert(r.key, r.freq);
        }
    }

    pub fn get_huffmap(&self) -> HashMap<i64, u64> {
        self.map.clone()
    }

    pub fn print_huffmap(&self) {
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
        bsize_bits: usize,
    ) -> (i64, i64) {
        let f = self.f.as_ref().unwrap();
        let bsize = 1 << bsize_bits;
        // dbg!((start, blocks, bsize));
        for bnum in 0..blocks {
            let from = start.0 + bnum * bsize;
            let to = from + bsize;
            let k: i64 = start.1 + bnum;
            let v = f.get_range(from as usize, to as usize);
            if bnum == blocks - 1 {
                println!(
                    "{},{} {}/{}:\t{}\t{},{}",
                    start.0,
                    bsize,
                    bnum + 1,
                    blocks,
                    v,
                    from,
                    to
                );
            }
            self.map.insert(k, v);
            if k > 0 && self.symmetric {
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
        let metadata = match qtype {
            ValueType::Raw => HKeyMetadata::Raw(self.get_raw_block(hkey).unwrap()),
            ValueType::Corrected => HKeyMetadata::Block(self.get_hkey_block(hkey).unwrap()),
        };
        let extra_bits = match metadata {
            HKeyMetadata::Raw(o) => o.bits,
            HKeyMetadata::Block(o) => o.block_size_bits,
            HKeyMetadata::None => 0,
        };

        HuffmanPartialKey {
            qtype,
            key: hkey,
            extra_bits,
            metadata,
        }
    }
    /// Given a raw or corrected value, output the hashmap key + extra bits.
    pub fn to_hkey(&self, diffvalue: DiffValue) -> HuffmanKey {
        match diffvalue.qtype {
            ValueType::Raw => HuffmanKey {
                qtype: ValueType::Raw,
                key: self.raw[0].key,
                extra_bits: self.raw[0].bits,
                extra_data: diffvalue.value,
                metadata: HKeyMetadata::Raw(self.raw[0]),
            },
            ValueType::Corrected => {
                let hsblock = self.get_qval_block(diffvalue.value).unwrap();
                hsblock.tr_corrected2hk(diffvalue.value)
            }
        }
    }
    /// Given a raw or corrected value, output the hashmap key + extra bits.
    pub fn from_hkey(&self, hkey: HuffmanKey) -> DiffValue {
        match hkey.qtype {
            ValueType::Raw => DiffValue {
                qtype: ValueType::Raw,
                value: hkey.extra_data,
            },
            ValueType::Corrected => {
                let hsblock = self.get_hkey_block(hkey.key).unwrap();
                DiffValue {
                    qtype: ValueType::Corrected,
                    value: hsblock.tr_hk2corrected(hkey),
                }
            }
        }
    }
    pub fn get_raw_block(&self, qval: i64) -> Option<HuffmanMapSRaw> {
        for blck in self.raw.iter() {
            if blck.key == qval {
                return Some(*blck);
            }
        }
        None
    }

    pub fn get_qval_block(&self, qval: i64) -> Option<HuffmanMapSBlock> {
        let qval = qval.abs();
        // dbg!(qval);
        for blck in self.blocks.iter() {
            if qval >= blck.start_quantized && qval < blck.end_quantized {
                return Some(*blck);
            }
        }
        None
    }
    pub fn get_hkey_block(&self, hkey: i64) -> Option<HuffmanMapSBlock> {
        let hkey = hkey.abs();
        for blck in self.blocks.iter() {
            if hkey >= blck.start_huffman && hkey < blck.end_huffman {
                return Some(*blck);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::super::weightfn::Sech2Fn;
    use super::HuffmanMapS;
    use super::WeightFn;

    #[test]
    fn test1() {
        let mut f = Sech2Fn::new(0.001, 1000000);
        f.compute_fn(16000);
        let mut m = HuffmanMapS::new(f);
        m.update_from_fn();
    }
}
