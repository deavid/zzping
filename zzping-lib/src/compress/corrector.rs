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

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum ValueType {
    Raw,
    Corrected,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub struct DiffValue {
    pub qtype: ValueType,
    pub value: i64,
}

impl DiffValue {
    pub fn new_raw(value: i64) -> Self {
        Self {
            qtype: ValueType::Raw,
            value,
        }
    }
    pub fn new_corrected(value: i64) -> Self {
        Self {
            qtype: ValueType::Corrected,
            value,
        }
    }
}

// Grabs predictor Option<f32> and i64 desired output and encodes the final value
pub struct BasicCorrector {}

impl BasicCorrector {
    pub fn new() -> Self {
        Self {}
    }
    pub fn diff(&self, qval: i64, predicted: Option<f64>) -> DiffValue {
        match predicted {
            None => DiffValue::new_raw(qval),
            Some(f) => {
                let f = f.round() as i64;
                DiffValue::new_corrected(qval - f)
            }
        }
    }
    pub fn undiff(&self, last_pred: Option<f64>, diff: DiffValue) -> i64 {
        match diff.qtype {
            ValueType::Raw => diff.value,
            ValueType::Corrected => {
                // If we don't have a prediction, yet this is corrected, panic.
                let f = last_pred.unwrap().round() as i64;
                diff.value + f
            }
        }
    }
}

impl Default for BasicCorrector {
    fn default() -> Self {
        Self {}
    }
}
