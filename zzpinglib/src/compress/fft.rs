use crate::dynrmp::variant::Variant;

use super::Compress;
use rustfft::num_traits::Zero;
use rustfft::{num_complex::Complex, FFTplanner};
use std::collections::HashMap;

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
    // TODO: This is a debugging part of quantizing!
    let mut freq: HashMap<u32, u32> = HashMap::new();
    for (m, _a) in quantized.iter() {
        *freq.entry(*m).or_insert(0) += 1;
    }
    let mut freq: Vec<(u32, u32)> = freq.into_iter().collect();
    freq.sort_unstable_by_key(|(k, v)| ((v << 12) + k) as i128 * -2);
    // dbg!(freq.len());
    // for (k, v) in freq.into_iter() {
    //     println!("{}:\t{}", k, v);
    // }
    // TODO: This is an inverse quantization step!
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

pub fn export_print(mid: &[f32]) {
    let fft = fft(&mid);
    let half_fft = half_fft(&fft);
    println!("output,--");
    print_polar(half_fft);
}

pub fn _print_vcplx(x: &[Complex<f32>]) {
    for v in x {
        println!("{:.9},{:.9}", v.re, v.im);
    }
}

pub fn print_polar(x: &[(f32, f32)]) {
    for (m, a) in x {
        println!("{:.9},{:.9}", m, a);
    }
}

#[derive(Debug)]
pub struct PolarCompress {
    pub data: Vec<(f32, f32)>,
    pub q_size_mag: usize, // Amount of different values for magnitude
    pub q_size_ang: usize, // Amount of different values for angle
    pub q_scale: f32,      // Scaling factor, 0.5 applies sqrt to input.
}

impl Default for PolarCompress {
    fn default() -> Self {
        Self {
            data: vec![],
            q_size_mag: 1 << 7,
            q_size_ang: (1 << 24) + 1,
            q_scale: 0.25,
        }
    }
}

impl PolarCompress {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Compress<f32> for PolarCompress {
    fn setup(&mut self, _params: HashMap<String, Variant>) {
        todo!()
    }

    fn compress(&mut self, data: &[f32]) {
        let fft_input = fft(data);
        let half_fft = half_fft(&fft_input);
        // In complex numbers, if we do f^(1/4) it should give us angles from -45º to 45º.
        // Multiply per 45º to get all values ranging from 0-N, 0i-Xi.
        // Since now all falls into the same range, we can do huffman with symbols (N+X)
        // using imaginary numbers and real numbers in the same dict.
        // They should have the same probability of landing on the same place.
        // The problem is anything on 180º will land on both 0º and 90º because it has two solutions.
        // The real number part will land in the 45º line.
        // https://docs.rs/huffman-compress/0.6.0/huffman_compress/
        let quantized_m = quantize(
            &half_fft
                .iter()
                .map(|(m, a)| (m.powf(self.q_scale), *a))
                .collect::<Vec<_>>(),
            self.q_size_mag,
            self.q_size_ang,
        );
        self.data = quantized_m;
    }

    fn serialize(&self) -> Vec<u8> {
        todo!()
    }

    fn deserialize(&mut self, _payload: &[u8]) {
        todo!()
    }

    fn decompress(&self) -> Vec<f32> {
        let data: Vec<(f32, f32)> = self
            .data
            .iter()
            .map(|(m, a)| (m.powf(1. / self.q_scale), *a))
            .collect();
        let dfft = double_fft(&data);
        inv_fft(&dfft)
    }
}
