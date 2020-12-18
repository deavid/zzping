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

use crate::compress::fft;
use crate::compress::quantize;
use crate::compress::Compress;
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

    pub fn test_recv_compression<T: Compress<f32>>(&self, mut zipper: T) {
        let mut data_len = self.recv_us.len();
        data_len -= data_len % 2; // FFT only allows for even amounts of data.

        let trasposed_recv = Self::transpose(&self.recv_us[..data_len]);
        let origdata = &trasposed_recv[3];

        zipper.compress(origdata);
        let unzipped = zipper.decompress();

        assert_eq!(unzipped.len(), origdata.len());

        Self::measure_error(&origdata, &unzipped);
    }

    pub fn transpose(data: &[[f32; 7]]) -> Vec<Vec<f32>> {
        let mut trasposed_recv: Vec<Vec<f32>> = vec![];
        for i in 0..7 {
            let perc: Vec<f32> = data.iter().map(|x| x[i]).collect();
            trasposed_recv.push(perc);
        }
        trasposed_recv
    }

    pub fn measure_error(origdata: &[f32], unzipped: &[f32]) {
        let mut error: f32 = 0.0;
        for (v, i) in origdata.iter().zip(unzipped) {
            //dbg!(v, i);
            let e = v - i;
            error += e * e;
        }
        let sum: f32 = origdata.iter().sum();
        let mean = sum / origdata.len() as f32;
        error /= origdata.len() as f32;
        error = error.sqrt();
        error *= 100.0;
        error /= mean;

        dbg!(error);
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
