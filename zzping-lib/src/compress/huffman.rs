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

/*
    Parameters:
    -----------------
    Input should be f32, not u32.
    Precision: 0.1% -> 1.01 -> ln(1.01)
    Min value: 100us -> ln(100)
    Max value: 30s   -> ln(30_000_000)
    Possible values: (Max - Min) / Precision =
        = (ln(30_000_000) - ln(100)) / ln(1.01) = 1267,4491

    Huffman symbol table encoding:
    --------------------------------
    Full symbol table + frequency: u16,u16
    Frequency only, inc. unused symbols: u16
    Frequency-encoding: i16, negative values do skip.
    Optional, frequency scaling to u8.

    Function based, i.e. tan(1/(x+10))

    Optional extra precision:
    -----------------------------
    Encode error as an extra i8 or i4.

    Other:
    -------------
    Quantization is common in all compression libraries. Common utilities might
    be useful.
    https://docs.rs/huffman-compress/0.6.0/huffman_compress/


*/

extern crate bit_vec;
extern crate huffman_compress;

use bit_vec::BitVec;
use huffman_compress::Book;
use huffman_compress::CodeBuilder;
use huffman_compress::Tree;
use std::collections::HashMap;
use std::fmt::Debug;
use std::iter::FromIterator;

use super::{Compress, CompressTo, Error};

#[derive(Debug)]
pub struct HuffmanQ<T: CompressTo<f32, u64> + Default> {
    pub quantizer: T,
    pub huffman: HuffmanU64,
}

impl<T: CompressTo<f32, u64> + Default> Default for HuffmanQ<T> {
    fn default() -> Self {
        Self {
            quantizer: T::default(),
            huffman: HuffmanU64::default(),
        }
    }
}

impl<T: CompressTo<f32, u64> + Default> Compress<f32> for HuffmanQ<T> {
    fn setup(
        &mut self,
        _params: std::collections::HashMap<String, crate::dynrmp::variant::Variant>,
    ) -> Result<(), Error> {
        Err(Error::ToDo)
    }

    fn compress(&mut self, data: &[f32]) -> Result<(), Error> {
        self.quantizer.compress(data)?;
        let quantizer_data = self.quantizer.get_data()?;
        self.huffman.compress(quantizer_data)?;
        Ok(())
    }

    fn serialize(&self) -> Result<Vec<u8>, Error> {
        Err(Error::ToDo)
    }

    fn deserialize(&mut self, _payload: &[u8]) -> Result<usize, Error> {
        Err(Error::ToDo)
    }

    fn decompress(&self) -> Result<Vec<f32>, Error> {
        let decoded: Vec<u64> = self.huffman.decompress()?;
        self.quantizer.decompress_from(&decoded)
    }
    fn debug_name(&self) -> String {
        format!("Huffman<{}>", self.quantizer.debug_name())
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

#[derive(Debug)]
pub struct HuffmanU64 {
    weights: Vec<(u64, u64)>,
    data: BitVec,
    data_len: usize,
    fuzzy: bool,
    max_count: u64,
}

impl HuffmanU64 {
    fn encode_weights(&self) {
        println!(
            "K: {:?}",
            self.weights.iter().map(|o| o.0).collect::<Vec<u64>>()
        );
        let mut buf: Vec<i64> = vec![];
        let mut pos: u64 = 0;
        for (k, v) in self.weights.iter().copied() {
            let mut d = k - pos;
            while d > 0 {
                println!("0");
                buf.push(0);
                d -= 1;
                // if d >= 64 {
                //         d -= 64;
                //         buf.push(-64);
                //     } else if d >= 16 {
                //         d -= 16;
                //         buf.push(-16);
                //     } else {
                //         buf.push(-(d as i64));
                //         d = 0;
                //     }
            }
            println!("{}", v);
            buf.push(v as i64);
            pos = k + 1;
        }
        println!("Diff: {:?}", buf);
        dbg!(buf.len());
    }
}

impl Default for HuffmanU64 {
    fn default() -> Self {
        Self {
            weights: vec![],
            data: BitVec::new(),
            data_len: 0,
            fuzzy: true,
            max_count: 3600,
        }
    }
}

impl Compress<u64> for HuffmanU64 {
    fn setup(
        &mut self,
        _params: std::collections::HashMap<String, crate::dynrmp::variant::Variant>,
    ) -> Result<(), Error> {
        Err(Error::ToDo)
    }

    fn compress(&mut self, data: &[u64]) -> Result<(), Error> {
        let quantizer_data = data;
        let mut weights: HashMap<u64, u64> = HashMap::new();
        for k in quantizer_data.iter() {
            *weights.entry(*k).or_insert(0) += 1;
        }
        // Double huffman encoding?
        let max_w = weights.values().max().copied().unwrap();
        let max_value = self.max_count;
        let q = max_w / max_value;
        if q > 1 {
            for (_, v) in weights.iter_mut() {
                *v += q - 1;
                *v /= q;
            }
        }
        let mut translate: HashMap<u64, u64> = HashMap::new(); // source->dest
        if self.fuzzy {
            let keys = weights.keys().copied().collect::<Vec<_>>();
            for k in keys {
                let v = weights[&k];
                if v == 1 {
                    let kl = k.overflowing_sub(1).0;
                    let kr = k + 1;
                    let new = weights
                        .get_key_value(&kr)
                        .or_else(|| weights.get_key_value(&kl));

                    if let Some(kv) = new {
                        if translate.get(kv.0).is_none() {
                            let dk: u64 = *kv.0;
                            translate.insert(k, dk);
                            *weights.get_mut(&k).unwrap() = 0;
                            *weights.get_mut(&dk).unwrap() += v;
                        }
                    }
                }
            }
        }
        for v in weights.values_mut() {
            *v = (*v).min(max_value - 1);
        }
        let max_w = weights.values().max().copied().unwrap();
        dbg!(max_w);

        self.weights = weights
            .iter()
            .map(|(k, v)| (*k, *v))
            .filter(|(_k, v)| *v > 0)
            .collect();

        self.weights.sort_unstable_by_key(|(k, _v)| *k);
        // println!(
        //     "K: {:?}",
        //     self.weights.iter().map(|o| o.0).collect::<Vec<u64>>()
        // );
        // println!(
        //     "V: {:?}",
        //     self.weights.iter().map(|o| o.1).collect::<Vec<u64>>()
        // );
        self.encode_weights();
        self.weights.sort_by_key(|(_k, v)| -(*v as i128));
        dbg!(self.weights.len());
        let (book, _tree) = CodeBuilder::from_iter(self.weights.iter().copied()).finish();
        self.data = BitVec::with_capacity(data.len() * 8);
        self.data_len = quantizer_data.len();
        for mut symbol in quantizer_data.iter() {
            if let Some(v) = translate.get(symbol) {
                symbol = v;
            }
            book.encode(&mut self.data, symbol)
                .map_err(Error::HuffmanEncodeError)?
        }
        dbg!(self.data.len() as f32 / data.len() as f32);
        // dbg!(total_bits);
        dbg!(self.data.len() / 8);
        Ok(())
    }

    fn serialize(&self) -> Result<Vec<u8>, Error> {
        Err(Error::ToDo)
    }

    fn deserialize(&mut self, _payload: &[u8]) -> Result<usize, Error> {
        Err(Error::ToDo)
    }

    fn decompress(&self) -> Result<Vec<u64>, Error> {
        let (_book, tree) = CodeBuilder::from_iter(self.weights.iter().copied()).finish();
        Ok(tree.decoder(&self.data, self.data_len).collect())
    }
    fn debug_name(&self) -> String {
        "Huffman<>".to_string()
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

#[derive(Debug)]
pub struct HuffmanI64 {
    book: Book<i64>,
    tree: Tree<i64>,
}

impl HuffmanI64 {
    pub fn new(weights: Vec<(i64, u64)>) -> Self {
        let (book, tree) = CodeBuilder::from_iter(weights.into_iter()).finish();
        Self { book, tree }
    }
    pub fn encode(&self, buffer: &mut BitVec, symbol: i64) -> Result<(), Error> {
        self.book
            .encode(buffer, &symbol)
            .map_err(Error::HuffmanEncodeError)
    }
    pub fn decode(&self, buffer: &mut bit_vec::Iter<u32>) -> Result<i64, Error> {
        self.tree
            .decoder(buffer, 1)
            .next()
            .ok_or(Error::HuffmanDecodeNoItemError)
    }
}
