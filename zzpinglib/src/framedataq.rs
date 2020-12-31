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

use chrono::{DateTime, NaiveDateTime, Utc};

#[derive(Debug, Clone)]
pub struct FrameDataQ {
    pub timestamp: Option<i64>,
    pub subsec_ms: u32,
    pub inflight: Vec<usize>,
    pub lost_packets: Vec<usize>,
    pub recv_us_len: Vec<usize>,
    pub recv_us: Vec<[f32; 7]>,
}

impl FrameDataQ {
    pub fn get_datetime(&self) -> DateTime<Utc> {
        let ts = self.timestamp.unwrap();
        let dt = NaiveDateTime::from_timestamp_opt(ts, self.subsec_ms).unwrap();
        DateTime::from_utc(dt, Utc)
    }
}
