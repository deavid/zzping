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

use zzpinglib::framedataq::FDCodecIter;

#[derive(Clap, Debug)]
#[clap(
    version = "0.2.0-beta1",
    author = "David Martinez Marti <deavidsedice@gmail.com>"
)]
struct Opts {
    #[clap(short, long)]
    input: String,
}

fn main() {
    let opts: Opts = Opts::parse();

    read_inputfile(&opts.input);
}

fn read_inputfile(filename: &str) {
    let f = File::open(filename).unwrap();
    let buf = BufReader::new(f);
    let fdreader = FDCodecIter::new(buf);
    for fdq in fdreader {
        println!("{}", fdq);
    }
}
