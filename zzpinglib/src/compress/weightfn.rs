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

pub trait WeightFn {
    fn compute_fn(&mut self, size: usize);
    fn get_fn(&self) -> Vec<f64>;
    fn get_range(&self, from: usize, to: usize) -> u64;
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
}

impl WeightFn for Sech2Fn {
    fn compute_fn(&mut self, size: usize) {
        self.function = Vec::with_capacity(size);
        let stdev = self.precision.recip().powf(0.97);
        dbg!(stdev);
        let items: f64 = self.item_count as f64;
        for i in 0..size {
            let v1: f64 = Self::sech2(i as f64 / stdev) * items;
            let v2: f64 = Self::sech2(i as f64 / stdev.powf(1.5)) * items.cbrt();
            let k: f64 = if i == 0 { 2.0 } else { 1.0 };
            self.function.push((v1 + v2) * k);
        }
    }
    fn get_fn(&self) -> Vec<f64> {
        self.function.clone()
    }
    fn get_range(&self, from: usize, to: usize) -> u64 {
        self.function[from..to].iter().sum::<f64>().ceil() as u64
    }
}

#[derive(Clone, Copy)]
pub struct RecipParams {
    pub stdev: f64,
    pub h: f64,
    pub k: f64,
}

pub static RP_INTERNET: [RecipParams; 2] = [
    RecipParams {
        stdev: 250.0,
        h: 4.0,
        k: 1500.0,
    },
    RecipParams {
        stdev: 1.0,
        h: 100.0,
        k: 3.0,
    },
];

pub static RP_LOCALNET: [RecipParams; 2] = [
    RecipParams {
        stdev: 150.0,
        h: 7.0,
        k: 1000.0,
    },
    RecipParams {
        stdev: 1.0,
        h: 100.0,
        k: 1.0,
    },
];

pub static RP_MIXED: [RecipParams; 2] = [
    RecipParams {
        stdev: 2.0,
        h: 100.0,
        k: 2500.0,
    },
    RecipParams {
        stdev: 1.0,
        h: 0.0,
        k: 250.0,
    },
];

pub static RP_DEFAULT: [RecipParams; 2] = RP_LOCALNET;
pub struct RecipFn {
    item_count: usize, // This is just for others to estimate / get on the same order of magnitude
    function: Vec<f64>,
    w: f64,
    f: [RecipParams; 2],
}

impl RecipFn {
    pub fn recip(v: f64, stdev: f64) -> f64 {
        let v = v.abs() + stdev;
        stdev / v
    }
    pub fn exp(v: f64, k: f64) -> f64 {
        let v = v.abs() / k;
        v.min(600.0).exp()
    }
    pub fn rfn(v: f64, p: &RecipParams) -> f64 {
        Self::recip(v, p.stdev) * p.h / Self::exp(v, p.k)
    }

    pub fn applyfn(&self, v: f64) -> f64 {
        let r = Self::rfn(v, &self.f[0]) + Self::rfn(v, &self.f[1]);
        if v.abs() < 1.0 {
            r * 5.0
        } else {
            r
        }
    }

    pub fn new(f: [RecipParams; 2], item_count: usize, w: f64) -> Self {
        Self {
            f,
            w,
            item_count,
            function: vec![],
        }
    }
}

impl WeightFn for RecipFn {
    fn compute_fn(&mut self, size: usize) {
        self.function = Vec::with_capacity(size);
        let items: f64 = self.item_count as f64;
        for i in 0..size {
            let v: f64 = self.applyfn(i as f64 * self.w) * items;
            self.function.push(v);
        }
    }
    fn get_fn(&self) -> Vec<f64> {
        self.function.clone()
    }
    fn get_range(&self, from: usize, to: usize) -> u64 {
        self.function[from..to].iter().sum::<f64>().ceil() as u64
    }
}

pub static MN_LOCALNET: [(i64, i64); 17] = [
    (0, 969000),
    (1, 75000),
    (2, 67000),
    (4, 75000),
    (8, 77500),
    (16, 72909),
    (32, 65174),
    (64, 53644),
    (128, 37286),
    (256, 16735),
    (512, 3022),
    (1024, 910),
    (2048, 865),
    (4096, 85),
    (8192, 5),
    (16384, 2),
    (32768, 0),
];

pub static MN_BASIC: [(i64, i64); 17] = [
    (0, 114730),
    (1, 22310),
    (2, 23820),
    (4, 26160),
    (8, 30110),
    (16, 53730),
    (32, 82060),
    (64, 64480),
    (128, 68140),
    (256, 60420),
    (512, 40290),
    (1024, 16520),
    (2048, 3470),
    (4096, 110),
    (8192, 0),
    (16384, 0),
    (32768, 0),
];

pub static MN_DEFAULT: [(i64, i64); 17] = MN_BASIC;

pub struct ManualFn {
    item_count: usize, // This is just for others to estimate / get on the same order of magnitude
    function: Vec<f64>,
    f: [(i64, i64); 17],
    max_f: i64,
}

impl ManualFn {
    pub fn applyfn(&self, v: f64) -> f64 {
        let v = v.abs();
        let lv = if v > 0.1 { v.log2() + 1.0 } else { 0.0 };
        let lvl = lv.floor() as usize;
        let lvr = lv.ceil() as usize;
        let lvlv = self.f[lvl.min(16)].1 as f64;
        let lvrv = self.f[lvr.min(16)].1 as f64;
        let dr = lv - lvl as f64;
        let dl = 1.0 - dr;
        (lvlv * dl + lvrv * dr) / self.max_f as f64
    }
    pub fn new(f: [(i64, i64); 17], item_count: usize) -> Self {
        let max_f = f.iter().map(|x| x.1).max().unwrap();

        Self {
            f,
            max_f,
            item_count,
            function: vec![],
        }
    }
    pub fn get_huffman_weights(&self, max_sz: i64, limit: i64) -> Vec<(i64, u64)> {
        let def_sz: i64 = 32768;
        let fn_max_h = self.max_f;
        let item_count: i64 = self.item_count as i64;
        let mut w: Vec<(i64, u64)> = Vec::with_capacity(limit as usize);
        for i in 0..limit {
            let v: f64 = i as f64 * def_sz as f64 / max_sz as f64;
            let a = self.applyfn(v) * item_count as f64 / fn_max_h as f64;
            w.push((i, a.round() as u64));
        }
        w
    }
}

impl WeightFn for ManualFn {
    fn compute_fn(&mut self, size: usize) {
        self.function = Vec::with_capacity(size);
        let items: f64 = self.item_count as f64;
        for i in 0..size {
            let v: f64 = self.applyfn(i as f64) * items;
            self.function.push(v);
        }
    }
    fn get_fn(&self) -> Vec<f64> {
        self.function.clone()
    }
    fn get_range(&self, from: usize, to: usize) -> u64 {
        self.function[from..to].iter().sum::<f64>().ceil() as u64
    }
}
