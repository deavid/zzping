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
use std::convert::TryInto;

use super::corrector::{BasicCorrector, ValueType};
use super::huffman::HuffmanI64;
use super::huffmapper::HuffmanMapS;
use super::predict::WindowMedianPredictor;
use super::quantize::LinearLogQuantizer;
// use super::weightfn::Sech2Fn;
// use super::weightfn::{RecipFn, RP_DEFAULT};
use super::weightfn::{ManualFn, MN_DEFAULT};
use super::Error;

pub struct CompositeStage {
    quantizer: LinearLogQuantizer,
    predictor: WindowMedianPredictor,
    correction: BasicCorrector,
    huffmapper: HuffmanMapS<ManualFn>,
    huffman: HuffmanI64,
    pub errors: Vec<i64>,
}

impl CompositeStage {
    pub fn new(precision: f64, window_size: usize) -> Self {
        let item_count = 100_000_000;
        // let f = Sech2Fn::new(precision, item_count);
        // let f = RecipFn::new(RP_DEFAULT, item_count, 1.0);
        let f = ManualFn::new(MN_DEFAULT, item_count);
        let hmaps = HuffmanMapS::new(f);
        let weights = hmaps.get_huffmap();
        Self {
            quantizer: LinearLogQuantizer::new(precision),
            predictor: WindowMedianPredictor::new(window_size),
            correction: BasicCorrector::new(),
            huffmapper: hmaps,
            huffman: HuffmanI64::new(weights.into_iter().collect()),
            errors: vec![],
        }
    }
    pub fn encode(&mut self, buffer: &mut BitVec, value: i64) {
        let qval = self.quantizer.encode(value);
        let predicted = self.predictor.predict_and_push(qval);
        let diff = self.correction.diff(qval, predicted);
        if diff.qtype == ValueType::Corrected {
            self.errors.push(diff.value);
        }
        let hkey = self.huffmapper.to_hkey(diff);
        // dbg!(hkey.key);
        self.huffman.encode(buffer, hkey.key).unwrap();
        let mut extra_data = hkey.encode_extra();
        buffer.append(&mut extra_data);
    }
    pub fn decode(&mut self, buffer: &mut bit_vec::Iter<u32>) -> Result<i64, Error> {
        // 1. read huffman symbol from buffer:
        let symbol = self.huffman.decode(buffer)?; // This actually marks the end of the input!
                                                   // 2. Determine the key type to get the number of bits
        let partial_key = self.huffmapper.get_partial_hkey(symbol);
        let extra_bits = partial_key.extra_bits;

        // 3. Read the extra data from buffer
        let extra_data: i64;
        if extra_bits > 0 {
            let mut bdata: BitVec = buffer.take(extra_bits).collect();
            let mut full_data = BitVec::from_elem(64 - extra_bits, false);
            full_data.append(&mut bdata);
            let vbytes: Vec<u8> = full_data.to_bytes();
            let abytes: [u8; 8] = vbytes.try_into().unwrap();
            extra_data = i64::from_be_bytes(abytes);
        } else {
            extra_data = 0;
        }
        // 3b. Get the huffmapper key
        let hkey = partial_key.add_extra_data(extra_data);

        // 4. Compose the original DiffValue
        let diff = self.huffmapper.from_hkey(hkey);

        // 5. Obtain the latest prediction (w/o push)
        let last_pred = self.predictor.predict();

        // 6. Use self.correction.undiff(predicted, correction) to get the original value
        let orig_qval = self.correction.undiff(last_pred, diff);

        // 7. Once qval is obtained, push that into the predictor.
        self.predictor.push_value(orig_qval);

        // 8. Quantizer.decode
        Ok(self.quantizer.decode(orig_qval))
    }
}

impl Default for CompositeStage {
    fn default() -> Self {
        let precision = 0.001;
        let window_size = 3;
        Self::new(precision, window_size)
    }
}

#[cfg(test)]
mod tests {
    use super::CompositeStage;
    use super::Error;

    #[test]
    fn test1() {
        let precision = 0.001;
        let window = 1;
        let mut cs_enc = CompositeStage::new(precision, window);
        let mut buffer = bit_vec::BitVec::new();
        let data = vec![100, 110, 120, 130, 125, 112, 115, 80, 155];
        for d in data.iter() {
            cs_enc.encode(&mut buffer, *d);
            dbg!(&buffer.len());
        }
        dbg!(&buffer);
        let mut cs_dec = CompositeStage::new(precision, window);
        let mut iter = buffer.iter();
        let mut new_data = vec![];
        // TODO: Stopping?
        for _ in 0..data.len() {
            let v = cs_dec.decode(&mut iter);
            match v {
                Ok(v) => new_data.push(v),
                Err(e) => {
                    dbg!(e);
                    break;
                }
            }
        }

        let v = cs_dec.decode(&mut iter);
        if let Err(e) = v {
            match e {
                Error::HuffmanDecodeNoItemError => (),
                _ => panic!("Unexpected error!"),
            }
        } else {
            panic!("Expected an error after consuming items!");
        }

        dbg!(buffer.len() as f32 / data.len() as f32);
        assert_eq!(data, new_data);
    }
}
