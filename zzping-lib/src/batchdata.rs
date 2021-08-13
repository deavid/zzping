// Copyright 2021 Google LLC
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

//! Experimental library for further compressing the data. DO NOT USE.

use chrono::{DateTime, Utc};

use crate::compress::{self, composite::CompositeStage, quantize::LinearLogQuantizer, Compress};
use crate::framedata::{FrameData, FrameTime};
use std::{collections::HashMap, convert::TryInto};

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

        // TODO: Delete ME.
        // let mut data_len = recv_us.len();
        // data_len -= data_len % 2; // FFT only allows for even amounts of data.

        // let trasposed_recv = Self::transpose(&recv_us[..data_len]);
        // let origdata = &trasposed_recv[3];

        // // Test Polar Compression:
        // let mut zipper = fft::PolarCompress::default();
        // zipper.compress(origdata);
        // let unzipped = zipper.decompress();
        // assert_eq!(unzipped.len(), origdata.len());

        // // Measure error
        // Self::measure_error(&origdata, &unzipped);

        // let mut quantizer = quantize::LogQuantizer::new();
        // quantizer.compress(&origdata);

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
    // ?Sized is required to use Box<dyn T>
    pub fn test_recv_compression<T: Compress<f32> + ?Sized>(
        &self,
        zipper: &mut T,
    ) -> Result<(), compress::Error> {
        let mut data_len = self.recv_us.len();
        data_len -= data_len % 2; // FFT only allows for even amounts of data.

        let trasposed_recv = Self::transpose(&self.recv_us[..data_len]);
        //let origdata = &trasposed_recv[3];
        let mut origdata: Vec<f32> = vec![];
        let mut oldvalue: f32 = 1.0;
        for v in trasposed_recv[3].iter() {
            origdata.push(v / oldvalue);
            oldvalue = *v;
        }

        zipper.compress(&origdata)?;
        let unzipped = zipper.decompress()?;

        if unzipped.len() != origdata.len() {
            dbg!(unzipped.len(), origdata.len());
            return Err(compress::Error::AssertError);
        }

        Self::measure_error(&origdata, &unzipped);
        Self::measure_window_error(&origdata, &unzipped);
        Ok(())
    }

    pub fn test_recv_composite_compression(&self, stats: &mut HashMap<i64, f64>) {
        let precision = 0.001;
        let window = 1;
        let mut cs_enc = CompositeStage::new(precision, window);
        let mut buffer = bit_vec::BitVec::new();
        let trasposed_recv = Self::transpose(&self.recv_us);
        let origdata: Vec<i64> = trasposed_recv[6].iter().map(|x| x.round() as i64).collect();
        for val in origdata.iter() {
            cs_enc.encode(&mut buffer, *val);
        }
        let stdev_err_sum: f32 = cs_enc.errors.iter().map(|x| (*x as f32).powi(2)).sum();
        let stdev_err = (stdev_err_sum / cs_enc.errors.len() as f32).sqrt();
        let mean_err_sum: f32 = cs_enc.errors.iter().map(|x| (*x as f32).abs()).sum();
        let mean_err = mean_err_sum / cs_enc.errors.len() as f32;
        let llq = LinearLogQuantizer::new(1.0);
        let mut sorted_errors: Vec<_> = cs_enc
            .errors
            .into_iter()
            .map(|x| llq.encode(x))
            .map(|x| llq.decode(x))
            .collect();
        sorted_errors.sort_unstable();
        let count_err = stats;

        for v in sorted_errors {
            let counter = count_err.entry(v).or_insert(0.0);
            let bcksz = llq.bucket_size(llq.encode(v));
            *counter += 1.0 / bcksz as f64;
        }

        dbg!(stdev_err, mean_err);
        dbg!(buffer.len() as f32 / origdata.len() as f32);

        let mut cs_dec = CompositeStage::new(precision, window);
        let mut buf_iter = buffer.iter();
        let mut dec_data: Vec<i64> = Vec::with_capacity(origdata.len());
        while let Ok(v) = cs_dec.decode(&mut buf_iter) {
            dec_data.push(v);
        }
        dbg!(origdata.len());
        dbg!(dec_data.len());

        let origdata_f32: Vec<f32> = origdata.iter().map(|x| *x as f32).collect();
        let dec_data_f32: Vec<f32> = dec_data.iter().map(|x| *x as f32).collect();
        Self::measure_error(&origdata_f32, &dec_data_f32);
        Self::measure_window_error(&origdata_f32, &dec_data_f32);

        // dbg!(count_err);
        // println!("data += {:?}", sorted_errors);
    }

    pub fn transpose(data: &[[f32; 7]]) -> Vec<Vec<f32>> {
        let mut trasposed_recv: Vec<Vec<f32>> = vec![];
        for i in 0..7 {
            let perc: Vec<f32> = data.iter().map(|x| x[i]).collect();
            trasposed_recv.push(perc);
        }
        trasposed_recv
    }

    pub fn flatten(data: &[[f32; 7]]) -> Vec<f32> {
        let mut flat_r: Vec<f32> = Vec::with_capacity(data.len() * 7);
        for row in data {
            for v in row {
                flat_r.push(*v);
            }
        }
        flat_r
    }

    pub fn unflatten(data: &[f32]) -> Vec<[f32; 7]> {
        assert_eq!(data.len() % 7, 0);
        let mut unflat_r: Vec<[f32; 7]> = Vec::with_capacity(data.len() / 7);

        for row in data.chunks_exact(7) {
            let row: [f32; 7] = row.try_into().unwrap();
            unflat_r.push(row);
        }
        unflat_r
    }

    pub fn measure_window_error(origdata: &[f32], unzipped: &[f32]) {
        let wide = 15;
        let mut werror: f32 = 0.0;
        let mut nans: usize = 0;
        let mut wmax_error: f32 = 0.0;
        for (vv, ii) in origdata.windows(wide).zip(unzipped.windows(wide)) {
            let v: f32 = vv.iter().sum();
            let i: f32 = ii.iter().sum();
            let mut e: f32 = (v - i) / wide as f32;
            wmax_error = wmax_error.max(100.0 * e.abs() / v);
            if e.is_nan() {
                e = v * v;
                nans += 1;
            } else {
                e = e * e;
            }
            werror += e;
        }
        let sum: f32 = origdata.iter().sum();
        let mean = sum / origdata.len() as f32;
        werror /= origdata.len() as f32;
        werror = werror.sqrt();
        werror *= 100.0;
        werror /= mean;

        dbg!(werror, wmax_error);
        if nans > 0 {
            dbg!(nans);
        }
    }

    pub fn measure_error(origdata: &[f32], unzipped: &[f32]) {
        let mut error: f32 = 0.0;
        let mut nans: usize = 0;
        let mut max_error: f32 = 0.0;
        for (v, i) in origdata.iter().zip(unzipped) {
            //dbg!(v, i);
            let mut e: f32 = v - i;
            max_error = max_error.max(100.0 * e.abs() / v);
            if e.is_nan() {
                e = v * v;
                nans += 1;
            } else {
                e = e * e;
            }
            error += e;
        }
        let sum: f32 = origdata.iter().sum();
        let mean = sum / origdata.len() as f32;
        error /= origdata.len() as f32;
        error = error.sqrt();
        error *= 100.0;
        error /= mean;

        dbg!(error, max_error);
        if nans > 0 {
            dbg!(nans);
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
}
