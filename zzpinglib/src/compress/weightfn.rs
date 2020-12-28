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
        let stdev = self.precision.recip().powf(1.0 / 2.0);
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
