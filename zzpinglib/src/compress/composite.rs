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
        }
    }
    pub fn decode(&mut self, buffer: &mut bit_vec::Iter<u32>) -> i64 {
        // 1. read huffman symbol from buffer:
        let symbol = self.huffman.decode(buffer).unwrap();

        // 2. Determine the key type to get the number of bits

        // let bits..

        // 3. Read the extra data from buffer

        // 4. Compose the original DiffValue

        // 5. Obtain the latest prediction (w/o push)

        // 6. Use self.correction.undiff(predicted, correction) to get the original value

        // 7. Once qval is obtained, push that into the predictor.

        // 8. Quantizer.decode
        0
    }
}

impl Default for CompositeStage {
    fn default() -> Self {
        let precision = 0.001;
        let window_size = 3;
        Self::new(precision, window_size)
    }
}
