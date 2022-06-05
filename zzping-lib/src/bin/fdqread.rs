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

use std::io::BufReader;
use std::{fs::File, io::Write};

use clap::Parser;

use zzping_lib::framedataq::{FDCodecState, IterFold, RMPCodec};
use zzping_lib::{
    compress::quantize::LinearLogQuantizer,
    framedataq::{FDCodecCfg, FDCodecIter},
};

#[derive(Parser, Debug)]
#[clap(
    version = "0.2.2-beta1",
    author = "David Martinez Marti <deavidsedice@gmail.com>"
)]
struct Opts {
    #[clap(short, long)]
    input: Vec<String>,
    #[clap(long, default_value = "1")]
    agg_step: usize,
    #[clap(long, default_value = "1")]
    agg_window: usize,

    // Save options
    #[clap(short, long)]
    output: Option<String>,
    #[clap(short, long)]
    quantize: Option<f64>,
    #[clap(short, long, default_value = "60")]
    time: i64,
    #[clap(short, long)]
    delta_enc: bool,
}

fn main() {
    let opts: Opts = Opts::parse();
    let mut obuffer = opts
        .output
        .map(|o| File::create(o).unwrap())
        .map(std::io::BufWriter::new);
    let quantizer = opts.quantize.map(LinearLogQuantizer::new);
    let interval = opts.time;
    let codeccfg = FDCodecCfg {
        full_encode_secs: interval,
        recv_llq: quantizer,
        delta_enc: opts.delta_enc,
    };
    let mut codec = FDCodecState::new(codeccfg);
    let header: Vec<u8> = FDCodecState::get_header(codeccfg);
    if let Some(buf) = obuffer.as_mut() {
        buf.write_all(&header).unwrap();
    }

    for filename in opts.input.iter() {
        let f = File::open(filename).unwrap();
        let buf = BufReader::new(f);
        let fdreader = FDCodecIter::new(buf);
        for fdq in fdreader.iter_fold(opts.agg_window, opts.agg_step) {
            match obuffer.as_mut() {
                Some(buf) => {
                    let fdq = codec.encode(fdq);
                    let rmp = fdq.to_rmp();
                    buf.write_all(&rmp).unwrap();
                }
                None => {
                    println!("{}", fdq);
                }
            }
        }
    }
}
