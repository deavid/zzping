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

use std::fs::File;
use std::io::BufReader;

use clap::Clap;

use zzpinglib::batchdata::BatchData;
use zzpinglib::compress::{fft, huffman, quantize};
use zzpinglib::framedata::FrameDataVec;

#[derive(Clap, Debug)]
#[clap(
    version = "0.1.0",
    author = "David Martinez Marti <deavidsedice@gmail.com>"
)]
struct Opts {
    #[clap(short, long)]
    input: String,
}

fn main() {
    let opts: Opts = Opts::parse();
    dbg!(&opts);

    let f = File::open(opts.input).unwrap();
    let mut reader = BufReader::new(f);
    let mut fdv = FrameDataVec::new();
    if let Err(e) = fdv.read(&mut reader, 100000) {
        dbg!(e);
    }
    dbg!(fdv.v.len());

    let bd = BatchData::new(fdv.v);
    bd.test_recv_compression(fft::PolarCompress::default());
    bd.test_recv_compression(quantize::LogQuantizer::default());
    /*dbg!(fdv.v.first());

    for v in fdv.v.iter() {
        let last_recv = &v.recv_us;
        // dbg!(&last_recv);
        // = &fdv.v.last().unwrap().recv_us;
        if last_recv.is_empty() {
            dbg!("empty");
            continue;
        }
        let midpt = (last_recv.len() - 1) / 2;
        let avg: u128 = last_recv.iter().copied().sum::<u128>() / last_recv.len() as u128;
        let median = last_recv[midpt];
        if avg > 100000 {
            println!("avg: {} \t median: {}", avg, median);
        }
        // let n: Vec<u128> = last_recv.iter().map(|v| *v * 64 / median).collect();
        // dbg!(n);
    }*/
}
