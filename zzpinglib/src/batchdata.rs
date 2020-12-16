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

use chrono::{DateTime, Utc};
use rustfft::num_complex::Complex;
use rustfft::num_traits::Zero;
use rustfft::FFTplanner;
use std::collections::HashMap;

use crate::framedata::{FrameData, FrameTime};

#[derive(Debug)]
pub struct BatchData {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub len: usize,
    pub inflight: Vec<usize>,
    pub lost_packets: Vec<usize>,
    pub recv_us_len: Vec<usize>,
    pub recv_us: Vec<[f32; 7]>,
}
// FrameData: 67.4 bytes / frame avg.
// target: 6-7 bytes / frame avg.

impl BatchData {
    pub fn new(v: Vec<FrameData>) -> Self {
        let ts = || {
            v.iter().filter_map(|x| match x.time {
                FrameTime::Timestamp(t) => Some(t),
                _ => None,
            })
        };
        // Start & End allow for fast seeks. (TODO: Add number of bytes to skip)
        let start = ts().min().unwrap();
        let end = ts().max().unwrap();
        // Len + start-end allow to estimate the avg elapsed time per frame.
        // It is also required to avoid storing the len several times later.
        let len = v.len();

        // Denormalizing allows for extra compression techniques.
        let inflight: Vec<usize> = v.iter().map(|x| x.inflight).collect();
        let lost_packets: Vec<usize> = v.iter().map(|x| x.lost_packets).collect();
        let recv_us_len: Vec<usize> = v.iter().map(|x| x.recv_us.len()).collect();
        let recv_us: Vec<[f32; 7]> = v
            .iter()
            .filter(|x| !x.recv_us.is_empty()) // This will require extra process to put the parts
            .map(|x| -> Vec<f32> { x.recv_us.iter().map(|x| (*x as f32)).collect() })
            .map(|x| Self::compute_percentiles(&x))
            .collect();

        // lost_packets could be compressed as i16:
        //      * >0: actual packet lost
        //      * <0: number of frames w/o packet loss.
        //      *  0: single frame, zero packet loss.
        let min_recv: f32 = recv_us
            .iter()
            .map(|x| x[0])
            .fold(99.9, |a, b| -> f32 { a.min(b) });
        let max_recv: f32 = recv_us
            .iter()
            .map(|x| x[6])
            .fold(-99.0, |a, b| -> f32 { a.max(b) });
        let wide_recv = max_recv - min_recv;
        let mut trasposed_recv: Vec<Vec<f32>> = vec![];
        for i in 0..7 {
            let perc: Vec<f32> = recv_us.iter().map(|x| x[i]).take(32670).collect();
            trasposed_recv.push(perc);
        }
        /*
        for mid in &trasposed_recv {
            let fft = Self::fft(&mid);
            let half_fft = Self::half_fft(&fft);
            println!("output,--");
            print_polar(half_fft);
        }*/
        let mid = &trasposed_recv[3];
        let fft = Self::fft(mid);
        // println!("output,--");
        let half_fft = Self::half_fft(&fft);
        // print_polar(half_fft);
        let quant_m = 1 << 7;
        let quant_a = (1 << 24) + 1;
        let pf: f32 = 0.25;
        // In complex numbers, if we do f^(1/4) it should give us angles from -45º to 45º.
        // Multiply per 45º to get all values ranging from 0-N, 0i-Xi.
        // Since now all falls into the same range, we can do huffman with symbols (N+X)
        // using imaginary numbers and real numbers in the same dict.
        // They should have the same probability of landing on the same place.
        // The problem is anything on 180º will land on both 0º and 90º because it has two solutions.
        // The real number part will land in the 45º line.
        // https://docs.rs/huffman-compress/0.6.0/huffman_compress/
        let quantized_m = Self::quantize(
            &half_fft
                .iter()
                .map(|(m, a)| (m.powf(pf), *a))
                .collect::<Vec<_>>(),
            quant_m,
            quant_a,
        )
        .iter()
        .map(|(m, a)| (m.powf(1. / pf), *a))
        .collect::<Vec<_>>();

        let dfft = Self::double_fft(&quantized_m);
        assert_eq!(fft.len(), dfft.len());

        let inv_fft = Self::inv_fft(&dfft);
        assert_eq!(inv_fft.len(), mid.len());

        let mut error: f32 = 0.0;
        for (v, i) in mid.iter().zip(inv_fft) {
            //dbg!(v, i);
            let e = v - i;
            error += e * e;
        }
        let sum: f32 = mid.iter().sum();
        let mean = sum / mid.len() as f32;
        error /= mid.len() as f32;
        error = error.sqrt();
        error *= 100.0;
        error /= mean;
        // fft   = 0.0002719643  (32b precision)
        // dfft  = 0.0002736262
        // 14,12 = 0.005799478   (13b precision)
        // 13,12 = 0.009756817
        // 13,11 = 0.011788452   (12b precision)
        // 12,10 = 0.02427861
        // 13,9  = 0.029825078
        // 12,8  = 0.06373
        // 10,8  = 0.09457038%   (9b precision)
        //  9,7  = 0.18387109%   (8b precision)
        //  8,8  = 0.2962995
        //  8,6  = 0.37499133
        //  6,4  = 1.5143014     (5b precision)
        //  0,0  = 8.976919

        dbg!(error);
        // recv_us can be transformed to ln(v)
        /*
        for v in recv_us.iter() {
            let v: Vec<u16> = v
                .iter()
                .map(|x| (x - min_recv) / wide_recv)
                .map(|x| (x * 65535.0) as u16)
                .collect();
            if v[0] > 0 {
                println!(
                    "{},{},{},{},{},{},{}",
                    v[0], v[1], v[2], v[3], v[4], v[5], v[6]
                );
            }
        }
        dbg!(&min_recv, &max_recv, &len);*/
        Self {
            start,
            end,
            len,
            inflight,
            lost_packets,
            recv_us_len,
            recv_us,
        }
    }

    pub fn compute_percentiles(v: &[f32]) -> [f32; 7] {
        let mut ret = [f32::NAN; 7];
        if v.is_empty() {
            return ret;
        }
        let percentiles = [0f32, 0.125, 0.25, 0.5, 0.75, 0.875, 1.0];
        let vmax = v.len() - 1;
        for (i, p) in percentiles.iter().enumerate() {
            let p = *p * vmax as f32;
            let (pl, pr) = (p.floor() as usize, p.ceil() as usize);
            if pl == pr {
                ret[i] = v[pl];
            } else {
                let fr = p - pl as f32;
                let fl = 1.0 - fr;
                ret[i] = v[pl] * fl + v[pr] * fr;
            }
        }
        ret
    }

    pub fn quantize(input: &[(f32, f32)], q_m: usize, q_a: usize) -> Vec<(f32, f32)> {
        let i = &input[1..];
        let min_m = i.iter().fold(f32::MAX, |acc, x| acc.min(x.0));
        let max_m = i.iter().fold(f32::MIN, |acc, x| acc.max(x.0));
        let min_a = i.iter().fold(f32::MAX, |acc, x| acc.min(x.1));
        let max_a = i.iter().fold(f32::MIN, |acc, x| acc.max(x.1));
        let range_m = max_m - min_m;
        let range_a = max_a - min_a;
        let vf = |x: &(f32, f32)| {
            (
                ((x.0 - min_m) * q_m as f32 / range_m).round() as u32,
                ((x.1 - min_a) * q_a as f32 / range_a).round() as u32,
            )
        };
        let quantized: Vec<(u32, u32)> = i.iter().map(vf).collect();
        let mut freq: HashMap<u32, u32> = HashMap::new();
        for (m, _a) in quantized.iter() {
            *freq.entry(*m).or_insert(0) += 1;
        }
        let mut freq: Vec<(u32, u32)> = freq.into_iter().collect();
        freq.sort_unstable_by_key(|(k, v)| ((v << 12) + k) as i128 * -2);
        dbg!(freq.len());
        for (k, v) in freq.into_iter() {
            println!("{}:\t{}", k, v);
        }

        let inv_vf = |x: (u32, u32)| {
            (
                (x.0 as f32 * range_m / q_m as f32) + min_m,
                (x.1 as f32 * range_a / q_a as f32) + min_a,
            )
        };
        input
            .iter()
            .take(1)
            .copied()
            .chain(quantized.into_iter().map(inv_vf))
            .collect()
    }

    pub fn fft(input_v: &[f32]) -> Vec<(f32, f32)> {
        let len = input_v.len();
        let lenf32 = len as f32;
        let mut input: Vec<Complex<f32>> = input_v
            .iter()
            .map(|x| Complex::new(*x / lenf32, 0.0))
            .collect();
        let mut output: Vec<Complex<f32>> = vec![Complex::zero(); len];
        let mut planner = FFTplanner::new(false);
        let fft = planner.plan_fft(len);
        fft.process(&mut input, &mut output);
        let polar: Vec<(f32, f32)> = output.into_iter().map(|x| x.to_polar()).collect();
        polar
    }

    pub fn inv_fft(input_v: &[(f32, f32)]) -> Vec<f32> {
        let len = input_v.len();
        let mut input: Vec<Complex<f32>> = input_v
            .iter()
            .map(|x| Complex::from_polar(x.0, x.1))
            .collect();
        let mut output: Vec<Complex<f32>> = vec![Complex::zero(); len];
        let mut planner = FFTplanner::new(true);
        let fft = planner.plan_fft(len);
        fft.process(&mut input, &mut output);
        let out: Vec<f32> = output.into_iter().map(|x| x.re).collect();
        out
    }
    pub fn half_fft(fft: &[(f32, f32)]) -> &[(f32, f32)] {
        let len = fft.len();
        let eps = 0.003;
        let mut errors = 0;
        assert!(len % 2 == 0); // Odd ffts do not have midpoints
        for i in 1..len / 2 {
            let l = fft[i];
            let r = fft[len - i];
            let d0 = l.0 - r.0;
            let d1 = l.1 + r.1;
            if d0.abs() > eps || d1.abs() > eps {
                errors += 1;
                println!("{}, {:.9} {:.9}", i, d0, d1);
            }
        }
        if errors > 0 {
            panic!("Found {} errors!", errors);
        }
        let mid = Complex::from_polar(fft[len / 2].0, fft[len / 2].1);
        if mid.im.abs() > eps {
            dbg!(fft[len / 2 - 1]);
            dbg!(fft[len / 2]);
            dbg!(fft[len / 2 + 1]);
            dbg!(mid);
            panic!("Mid element should be real with no imaginary part!");
        }
        &fft[0..len / 2 + 1]
    }
    pub fn double_fft(half_fft: &[(f32, f32)]) -> Vec<(f32, f32)> {
        // This only works for even ffts
        let mut fft = half_fft.to_vec();
        let len = fft.len();
        for i in (1..len - 1).rev() {
            let l = fft[i];
            let r = (l.0, -l.1);
            fft.push(r);
        }
        fft
    }
}

fn _print_vcplx(x: &[Complex<f32>]) {
    for v in x {
        println!("{:.9},{:.9}", v.re, v.im);
    }
}

fn print_polar(x: &[(f32, f32)]) {
    for (m, a) in x {
        println!("{:.9},{:.9}", m, a);
    }
}
