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

#[allow(unused_imports)]
use zzpinglib::compress::{fft, huffman, quantize};

use zzpinglib::framedata::{FrameData, FrameDataVec};
use zzpinglib::{batchdata::BatchData, compress::Compress};

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

    // test_batchdata_compression(fdv.v);
    test_serializer(fdv.v);
}

#[allow(dead_code)]
fn test_batchdata_compression(v: Vec<FrameData>) {
    let bd = BatchData::new(v);

    let tests: Vec<Box<dyn Compress<f32>>> = vec![
        // Box::new(fft::PolarCompress::default()),
        // Box::new(quantize::LogQuantizer::default()),
        // Box::new(quantize::LinearQuantizer::default()),
        Box::new(huffman::HuffmanQ::<quantize::LogQuantizer>::default()),
        //Box::new(huffman::HuffmanQ::<quantize::LinearQuantizer>::default()),
        //Box::new(fft::FFTCmplxCompress::default()),
        // Box::new(fft::FFTPolarCompress::default()),
    ];
    for mut t in tests {
        dbg!(t.debug_name());
        if let Err(e) = bd.test_recv_compression(t.as_mut()) {
            dbg!(e);
        }
    }
}

#[allow(dead_code)]
fn test_serializer(v: Vec<FrameData>) {
    let bd = BatchData::new(v);
    let trasposed_recv = BatchData::transpose(&bd.recv_us);
    let test_vec = &trasposed_recv[3];

    let mut serializer = quantize::LogQuantizer::default();
    serializer.compress(test_vec).unwrap();
    let ser_data = serializer.serialize().unwrap();
    dbg!(ser_data.len());
    dbg!(ser_data.len() as f32 * 8.0 / test_vec.len() as f32);

    let mut deserializer = quantize::LogQuantizer::default();
    dbg!("Deserialize");
    deserializer.deserialize(&ser_data).unwrap();
    dbg!("Decompress");
    let unzipped = deserializer.decompress().unwrap();
    dbg!(unzipped.len());
    assert_eq!(test_vec.len(), unzipped.len());
    let mut errors = 0;
    for (n, (s, d)) in serializer
        .data
        .iter()
        .zip(deserializer.data.iter())
        .enumerate()
    {
        if *s != *d {
            if errors < 10 {
                println!("{}: {} != {}", n, s, d);
            }
            errors += 1;
        }
    }
    if errors > 0 {
        dbg!(errors, serializer.data.len());
    }

    // bd.
}
