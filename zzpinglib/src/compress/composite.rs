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

use super::corrector::BasicCorrector;
use super::huffman::HuffmanI64;
use super::huffmapper::HuffmanMapS;
use super::predict::WindowMedianPredictor;
use super::quantize::LinearLogQuantizer;
use super::weightfn::Sech2Fn;

pub struct CompositeStage {
    quantizer: LinearLogQuantizer,
    predictor: WindowMedianPredictor,
    correction: BasicCorrector,
    huffmapper: HuffmanMapS,
    huffman: HuffmanI64,
}

impl CompositeStage {
    pub fn new(precision: f64, window_size: usize) -> Self {
        let item_count = 1000000;
        let f = Sech2Fn::new(precision, item_count);
        let hmaps = HuffmanMapS::new(f);
        let weights = hmaps.get_huffmap();
        Self {
            quantizer: LinearLogQuantizer::new(precision),
            predictor: WindowMedianPredictor::new(window_size),
            correction: BasicCorrector::new(),
            huffmapper: hmaps,
            huffman: HuffmanI64::new(weights.into_iter().collect()),
        }
    }
    pub fn encode(&mut self, buffer: &mut BitVec, value: i64) {
        let qval = self.quantizer.encode(value);
        let predicted = self.predictor.predict_and_push(qval);
        let diff = self.correction.diff(qval, predicted);
        let hkey = self.huffmapper.to_hkey(diff);

        self.huffman.encode(buffer, hkey.key).unwrap();
        if hkey.extra_bits > 0 {
            // Encode now the extra bits into buffer!
            let extra: [u8; 8] = hkey.extra_data.to_be_bytes();
            let mut extravec = BitVec::from_bytes(&extra);
            let left_bit = 64 - hkey.extra_bits;
            let mut rhs = extravec.split_off(left_bit);
            buffer.append(&mut rhs)
        }
    }
    pub fn decode(&mut self, buffer: &mut bit_vec::Iter<u32>) -> i64 {
        // 1. read huffman symbol from buffer:
        let symbol = self.huffman.decode(buffer).unwrap();

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
        self.quantizer.decode(orig_qval)
    }
}

impl Default for CompositeStage {
    fn default() -> Self {
        let precision = 0.001;
        let window_size = 3;
        Self::new(precision, window_size)
    }
}
