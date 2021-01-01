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
use std::io::{BufReader, Write};
use std::thread;

use clap::Clap;

use zzpinglib::framedataq::{FDCodecCfg, FrameDataQ, RMPCodec};
use zzpinglib::{compress::quantize::LinearLogQuantizer, framedataq::FDCodecState};
use zzpinglib::{framedata::FrameDataVec, framedataq::Complete};

#[derive(Clap, Debug)]
#[clap(
    version = "0.1.1",
    author = "David Martinez Marti <deavidsedice@gmail.com>"
)]
struct Opts {
    #[clap(short, long)]
    input: Vec<String>,
    #[clap(short, long)]
    output: Option<String>,
    #[clap(short, long)]
    quantize: Option<f64>,
    #[clap(short, long)]
    time: Option<i64>,
}

fn main() {
    let opts: Opts = Opts::parse();
    let mut obuffer = opts
        .output
        .map(|o| File::create(o).unwrap())
        .map(std::io::BufWriter::new);

    let mut handles = vec![];
    let quantizer = opts.quantize.map(LinearLogQuantizer::new);
    let interval = opts.time.unwrap_or(60);

    for input in opts.input {
        let handle = thread::spawn(move || read_inputfile(&input, quantizer, interval));
        handles.push(handle);
        if handles.len() > 7 {
            let data = handles.remove(0).join().unwrap();
            if let Some(buf) = obuffer.as_mut() {
                buf.write_all(&data).unwrap();
            }
        }
    }
    for handle in handles {
        let data = handle.join().unwrap();
        if let Some(buf) = obuffer.as_mut() {
            buf.write_all(&data).unwrap();
        }
    }
}

fn read_inputfile(filename: &str, quantizer: Option<LinearLogQuantizer>, interval: i64) -> Vec<u8> {
    let f = File::open(filename).unwrap();
    let mut reader = BufReader::new(f);
    let mut fdv = FrameDataVec::new();
    if fdv.read(&mut reader, 100000).is_err() {
        // dbg!(e);
    }
    let mut codec = FDCodecState::new(FDCodecCfg {
        full_encode_secs: interval,
        recv_llq: quantizer,
    });
    let mut buf = Vec::with_capacity(fdv.v.len() * 12);
    for frame in fdv.v.iter() {
        let fdq: FrameDataQ<Complete> = FrameDataQ::from_framedata(frame);
        let fdq = codec.encode(fdq);
        let mut rmp = fdq.to_rmp();
        buf.append(&mut rmp);
    }
    buf
}
