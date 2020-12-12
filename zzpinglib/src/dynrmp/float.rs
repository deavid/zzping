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

use std::{
    cmp::Ordering,
    hash::{Hash, Hasher},
};

#[derive(Default, Debug)]
pub struct Float {
    pub v: f64,
}

impl Float {
    pub fn new(v: f64) -> Self {
        Self { v }
    }

    pub fn as_f64(&self) -> f64 {
        self.v
    }
}

impl Hash for Float {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let s = format!("{:?}f", self.v);
        s.hash(state);
    }
}

impl PartialEq for Float {
    fn eq(&self, other: &Self) -> bool {
        self.v == other.v
    }
}

impl PartialOrd for Float {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.v.partial_cmp(&other.v)
    }
}

impl Ord for Float {
    fn cmp(&self, other: &Self) -> Ordering {
        // The problem here are the special values.
        // Ideally we would impose "strict" ordering, like placing NaNs first or last.
        // Basically, if these numbers were in a database or spreadsheet, how do
        // you want them ordered? ; for now, this is just wrong.
        self.v
            .partial_cmp(&other.v)
            .unwrap_or_else(|| self.v.to_bits().cmp(&other.v.to_bits()))
    }
}

impl Eq for Float {}
