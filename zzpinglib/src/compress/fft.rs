use crate::dynrmp::variant::Variant;

use super::{huffman, quantize, Compress, CompressTo, Error};
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

pub fn fft_cmplx(input_v: &[f32]) -> Vec<Complex<f32>> {
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
    output
}

pub fn inv_fft_cmplx(input_v: &[Complex<f32>]) -> Vec<f32> {
    let len = input_v.len();
    let mut input: Vec<Complex<f32>> = input_v.to_vec();
    let mut output: Vec<Complex<f32>> = vec![Complex::zero(); len];
    let mut planner = FFTplanner::new(true);
    let fft = planner.plan_fft(len);
    fft.process(&mut input, &mut output);
    let out: Vec<f32> = output.into_iter().map(|x| x.re).collect();
    out
}

pub fn fft_polar(input_v: &[f32]) -> Vec<(f32, f32)> {
    let polar: Vec<(f32, f32)> = fft_cmplx(input_v)
        .into_iter()
        .map(|x| x.to_polar())
        .collect();
    polar
}

pub fn inv_fft_polar(input_v: &[(f32, f32)]) -> Vec<f32> {
    let input: Vec<Complex<f32>> = input_v
        .iter()
        .map(|x| Complex::from_polar(x.0, x.1))
        .collect();
    inv_fft_cmplx(&input)
}

pub fn half_fft_cmplx(fft: &[Complex<f32>]) -> &[Complex<f32>] {
    let len = fft.len();
    let eps = 0.003;
    let mut errors = 0;
    assert!(len % 2 == 0); // Odd ffts do not have midpoints
    for i in 1..len / 2 {
        let l = fft[i];
        let r = fft[len - i];
        let d0 = l.re - r.re;
        let d1 = l.im + r.im;
        if d0.abs() > eps || d1.abs() > eps {
            errors += 1;
            println!("{}, {:.9} {:.9}", i, d0, d1);
        }
    }
    if errors > 0 {
        panic!("Found {} errors!", errors);
    }
    let mid = fft[len / 2];
    if mid.im.abs() > eps {
        dbg!(fft[len / 2 - 1]);
        dbg!(fft[len / 2]);
        dbg!(fft[len / 2 + 1]);
        dbg!(mid);
        panic!("Mid element should be real with no imaginary part!");
    }
    &fft[0..len / 2 + 1]
}

pub fn half_fft_polar(fft: &[(f32, f32)]) -> &[(f32, f32)] {
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
pub fn double_fft_polar(half_fft: &[(f32, f32)]) -> Vec<(f32, f32)> {
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

pub fn double_fft_cmplx(half_fft: &[Complex<f32>]) -> Vec<Complex<f32>> {
    // This only works for even ffts
    let mut fft = half_fft.to_vec();
    let len = fft.len();
    for i in (1..len - 1).rev() {
        let l = fft[i];
        let r = Complex::new(l.re, -l.im);
        fft.push(r);
    }
    fft
}

pub fn export_print(mid: &[f32]) {
    let fft = fft_polar(&mid);
    let half_fft = half_fft_polar(&fft);
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
            q_size_mag: 1 << 9,
            q_size_ang: (1 << 8) + 1,
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
    fn setup(&mut self, _params: HashMap<String, Variant>) -> Result<(), Error> {
        Err(Error::ToDo)
    }

    fn compress(&mut self, data: &[f32]) -> Result<(), Error> {
        let fft_input = fft_polar(data);
        let half_fft = half_fft_polar(&fft_input);

        let quantized_m = quantize(
            &half_fft
                .iter()
                .map(|(m, a)| (m.powf(self.q_scale), *a))
                .collect::<Vec<_>>(),
            self.q_size_mag,
            self.q_size_ang,
        );
        self.data = quantized_m;
        Ok(())
    }

    fn serialize(&self) -> Result<Vec<u8>, Error> {
        Err(Error::ToDo)
    }

    fn deserialize(&mut self, _payload: &[u8]) -> Result<(), Error> {
        Err(Error::ToDo)
    }

    fn decompress(&self) -> Result<Vec<f32>, Error> {
        let data: Vec<(f32, f32)> = self
            .data
            .iter()
            .map(|(m, a)| (m.powf(1. / self.q_scale), *a))
            .collect();
        let dfft = double_fft_polar(&data);
        Ok(inv_fft_polar(&dfft))
    }

    fn debug_name(&self) -> String {
        format!(
            "PolarCompress<qsm:{}, qsa:{}, qsc:{}>",
            self.q_size_mag, self.q_size_ang, self.q_scale
        )
    }

    fn serialize_metadata(&self) -> Result<Vec<u8>, Error> {
        todo!()
    }

    fn serialize_data(&self) -> Result<Vec<u8>, Error> {
        todo!()
    }

    fn deserialize_metadata(&mut self, _payload: &[u8]) -> Result<(), Error> {
        todo!()
    }

    fn deserialize_data(&mut self, _payload: &[u8]) -> Result<(), Error> {
        todo!()
    }
}

impl CompressTo<f32, u64> for PolarCompress {
    fn get_data(&self) -> Result<&[u64], Error> {
        // Problem: self.data is (f32, f32) magnitude, angle.
        // This expects u64.
        // TODO: Step 1 - These values are actually quantized but in float form.
        //      .. undo the inv_quantization or split it.
        // TODO: Step 2 - This applies to the PolarCompress (should be named PolarFFT)
        //      .. ComplexFFT can actually be flattened from (u64, u64) to u64.
        // Ok(&self.data)
        todo!()
    }

    fn decompress_from(&self, _srcdata: &[u64]) -> Result<Vec<f32>, Error> {
        // Problem: self.data is (f32, f32) magnitude, angle.
        // This expects u64.
        // self.decompress_data(&srcdata)
        todo!()
    }
}

// In complex numbers, if we do f^(1/4) it should give us angles from -45º to 45º.
// Multiply per 45º to get all values ranging from 0-N, 0i-Xi.
// Since now all falls into the same range, we can do huffman with symbols (N+X)
// using imaginary numbers and real numbers in the same dict.
// They should have the same probability of landing on the same place.
// The problem is anything on 180º will land on both 0º and 90º because it has two solutions.
// The real number part will land in the 45º line.

#[derive(Debug)]
pub struct FFTCmplxCompress {
    pub huffman: huffman::HuffmanQ<quantize::LinearQuantizer>,
    pub turn: Complex<f32>,
    pub complex_pow: f32,
    pub float_pow: f32,
    pub eps: f32,
}

impl Default for FFTCmplxCompress {
    fn default() -> Self {
        let mut huffman = huffman::HuffmanQ::<quantize::LinearQuantizer>::default();
        // huffman.quantizer.precision = 0.01;
        huffman.quantizer.max_value = 100000;
        Self {
            huffman,
            turn: Complex::from_polar(1.0, 0.0),
            complex_pow: 1.0,
            float_pow: 1.0,
            eps: 0.0,
            // turn: Complex::from_polar(1.0, std::f32::consts::PI / 4.0),
            // complex_pow: 4.0,
            // float_pow: 0.1,
            // eps: 0.01,
        }
    }
}
impl Compress<f32> for FFTCmplxCompress {
    fn setup(&mut self, _params: HashMap<String, Variant>) -> Result<(), Error> {
        Err(Error::ToDo)
    }

    fn compress(&mut self, data: &[f32]) -> Result<(), Error> {
        let fft_input = fft_cmplx(data);
        let half_fft: Vec<Complex<f32>> = half_fft_cmplx(&fft_input)
            .iter()
            .map(|c| c.powf(1.0 / self.complex_pow)) // Raise to 1/4th, maps to -45º to 45º
            .map(|c| c * self.turn) // Turn 45 degrees, so ends in 0-90º
            .collect();
        let mut zipped: Vec<f32> = Vec::with_capacity(half_fft.len() * 2);
        // dbg!(&half_fft[..100]);
        for c in half_fft {
            zipped.push((c.re + self.eps).powf(1.0 / self.float_pow));
            zipped.push((c.im + self.eps).powf(1.0 / self.float_pow));
        }
        self.huffman.compress(&zipped)?;
        Ok(())
    }

    fn serialize(&self) -> Result<Vec<u8>, Error> {
        Err(Error::ToDo)
    }

    fn deserialize(&mut self, _payload: &[u8]) -> Result<(), Error> {
        Err(Error::ToDo)
    }

    fn decompress(&self) -> Result<Vec<f32>, Error> {
        let unzipped = self.huffman.decompress()?;
        let half_fft: Vec<Complex<f32>> = unzipped
            .chunks_exact(2)
            .map(|c| {
                Complex::<f32>::new(
                    c[0].powf(self.float_pow) - self.eps,
                    c[1].powf(self.float_pow) - self.eps,
                )
            })
            .map(|c| c / self.turn)
            .map(|c| c.powf(self.complex_pow))
            .collect();
        let fft = double_fft_cmplx(&half_fft);
        Ok(inv_fft_cmplx(&fft))
    }

    fn debug_name(&self) -> String {
        format!("FFtCmplxCompress<{}>", self.huffman.debug_name())
    }

    fn serialize_metadata(&self) -> Result<Vec<u8>, Error> {
        todo!()
    }

    fn serialize_data(&self) -> Result<Vec<u8>, Error> {
        todo!()
    }

    fn deserialize_metadata(&mut self, _payload: &[u8]) -> Result<(), Error> {
        todo!()
    }

    fn deserialize_data(&mut self, _payload: &[u8]) -> Result<(), Error> {
        todo!()
    }
}

#[derive(Debug)]
pub struct FFTPolarCompress {
    pub huffman_r: huffman::HuffmanQ<quantize::LogQuantizer>,
    pub huffman_t: huffman::HuffmanQ<quantize::LinearQuantizer>,
    pub min_lin_f: f32,
}

impl FFTPolarCompress {
    pub fn get_linear_factor(&self, n: usize, l: usize) -> f32 {
        let n = n as f32;
        let l = l as f32;
        let f: f32 = n / l; // Converts to 0..1
        let f1 = 1.0 - f; // to 1..0
        let ff = f1 + self.min_lin_f;
        ff / (1.0 + self.min_lin_f)
    }

    pub fn linear_factor(&self, n: usize, l: usize, polar: (f32, f32)) -> (f32, f32) {
        let ff1 = self.get_linear_factor(n, l);
        (polar.0 * ff1, polar.1 * ff1)
    }

    pub fn inv_linear_factor(&self, n: usize, l: usize, polar: (f32, f32)) -> (f32, f32) {
        let ff1 = self.get_linear_factor(n, l);
        (polar.0 / ff1, polar.1 / ff1)
    }
}

impl Default for FFTPolarCompress {
    fn default() -> Self {
        let mut huffman_r = huffman::HuffmanQ::<quantize::LogQuantizer>::default();
        let mut huffman_t = huffman::HuffmanQ::<quantize::LinearQuantizer>::default();
        huffman_r.quantizer.precision = 0.01;
        huffman_t.quantizer.max_value = 10000;
        Self {
            huffman_r,
            huffman_t,
            min_lin_f: 1.0,
        }
    }
}
impl Compress<f32> for FFTPolarCompress {
    fn setup(&mut self, _params: HashMap<String, Variant>) -> Result<(), Error> {
        Err(Error::ToDo)
    }

    fn compress(&mut self, data: &[f32]) -> Result<(), Error> {
        let fft_input = fft_polar(data);
        let half_fft: &[(f32, f32)] = half_fft_polar(&fft_input);
        let len = half_fft.len();
        let hf_adj: Vec<(f32, f32)> = half_fft
            .iter()
            .copied()
            .enumerate()
            .map(|(i, p)| self.linear_factor(i, len, p))
            .collect();
        let mut radius: Vec<f32> = Vec::with_capacity(len);
        let mut theta: Vec<f32> = Vec::with_capacity(len);
        for (r, t) in hf_adj {
            radius.push(r);
            theta.push(t);
        }
        self.huffman_r.compress(&radius)?;
        self.huffman_t.compress(&theta)?;
        Ok(())
    }

    fn serialize(&self) -> Result<Vec<u8>, Error> {
        Err(Error::ToDo)
    }

    fn deserialize(&mut self, _payload: &[u8]) -> Result<(), Error> {
        Err(Error::ToDo)
    }

    fn decompress(&self) -> Result<Vec<f32>, Error> {
        let radius = self.huffman_r.decompress()?;
        let theta = self.huffman_t.decompress()?;
        let polar_half_fft: Vec<(f32, f32)> = radius.into_iter().zip(theta.into_iter()).collect();
        let len = polar_half_fft.len();
        let polar_half_adj: Vec<(f32, f32)> = polar_half_fft
            .into_iter()
            .enumerate()
            .map(|(i, p)| self.inv_linear_factor(i, len, p))
            .collect();
        let polar_fft = double_fft_polar(&polar_half_adj);
        let unzipped = inv_fft_polar(&polar_fft);

        Ok(unzipped)
    }

    fn debug_name(&self) -> String {
        format!(
            "FFtCmplxCompress<r:{},t:{}>",
            self.huffman_r.debug_name(),
            self.huffman_t.debug_name()
        )
    }

    fn serialize_metadata(&self) -> Result<Vec<u8>, Error> {
        todo!()
    }

    fn serialize_data(&self) -> Result<Vec<u8>, Error> {
        todo!()
    }

    fn deserialize_metadata(&mut self, _payload: &[u8]) -> Result<(), Error> {
        todo!()
    }

    fn deserialize_data(&mut self, _payload: &[u8]) -> Result<(), Error> {
        todo!()
    }
}
