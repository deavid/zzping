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

// Predictors will need to output f64 in order to be able to mix them together
// with precision.
// Actually, they should return Option<f64>

// Takes last N inputs and predicts an output based on the Median
pub struct WindowMedianPredictor {
    window_size: usize, // How many values to look back
    buffer: Vec<i64>,   // List of past values
}

impl WindowMedianPredictor {
    pub fn new(window_size: usize) -> Self {
        Self {
            window_size,
            buffer: vec![],
        }
    }
    pub fn push_value(&mut self, value: i64) {
        // TODO: This is inefficient. Use a ring-buffer for this.
        self.buffer.push(value);
        if self.buffer.len() > self.window_size {
            self.buffer.remove(0);
        }
    }
    pub fn predict(&mut self) -> Option<f64> {
        let blen = self.buffer.len();
        match blen {
            0 => None,
            1 => Some(self.buffer[0] as f64),
            _ => {
                let mut v = self.buffer.clone();
                v.sort_unstable();
                let idx = (blen as f64 - 1.0) / 2.0;
                let i1: usize = idx.floor() as usize;
                let i2: usize = idx.ceil() as usize;

                let f: f64 = (v[i1] + v[i2]) as f64 / 2.0;
                Some(f)
            }
        }
    }
}

// Takes other recently decoded values + their past values to predict
pub struct MultivariatePredictor {}

// Mixed predictor?? (combining several predictors together)
pub struct MixPredictor {}
