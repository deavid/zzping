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
            // .filter(|x| !x.recv_us.is_empty())
            .map(|x| -> Vec<f32> { x.recv_us.iter().map(|x| (*x as f32).ln()).collect() })
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
        // recv_us can be transformed to ln(v)
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
        dbg!(&min_recv, &max_recv, &len);
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
}
