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

use std::io::{BufReader, Write};
use std::sync::{Arc, Mutex};
use std::thread;
use std::{collections::HashMap, fs::File};

use bit_vec::BitVec;
use clap::Clap;

use quantize::LinearLogQuantizer;
#[allow(unused_imports)]
use zzpinglib::compress::{fft, huffman, quantize};

use zzpinglib::{
    batchdata::BatchData,
    compress::{corrector::DiffValue, huffman::HuffmanI64, weightfn::MN_BASIC, Compress},
};
use zzpinglib::{
    compress::{huffmapper::HuffmanMapS, weightfn::ManualFn},
    framedata::{FrameData, FrameDataVec},
};

#[derive(Clap, Debug)]
#[clap(
    version = "0.2.0-beta1",
    author = "David Martinez Marti <deavidsedice@gmail.com>"
)]
struct Opts {
    #[clap(short, long)]
    input: Vec<String>,
    #[clap(short, long)]
    output: Option<String>,
}

fn main() {
    let opts: Opts = Opts::parse();
    //    dbg!(&opts);
    let mut total_data: Vec<[i64; 7]> = vec![];
    let stats: HashMap<i64, f64> = HashMap::new();
    let mstats = Arc::new(Mutex::new(stats));
    let mut handles = vec![];
    for input in opts.input {
        let mstats = mstats.clone();
        let handle = thread::spawn(move || {
            let mut stats: HashMap<i64, f64> = HashMap::new();
            let data: Vec<[i64; 7]> = read_inputfile(&input, &mut stats);
            let mut ostats = mstats.lock().unwrap();
            for (k, v) in stats {
                *ostats.entry(k).or_insert(0.0) += v;
            }
            data
        });
        handles.push(handle);
        if handles.len() > 8 {
            let mut data = handles.remove(0).join().unwrap();
            total_data.append(&mut data);
        }
    }
    for handle in handles {
        let mut data = handle.join().unwrap();
        total_data.append(&mut data);
    }

    let stats: HashMap<_, _> = Arc::try_unwrap(mstats).unwrap().into_inner().unwrap();
    let mut stats: Vec<_> = stats.into_iter().collect();
    stats.sort_unstable_by_key(|x| x.0);
    for (n, (k, v)) in stats.iter().enumerate() {
        print!("{}: {:.3}, ", k, v);
        if n % 10 == 9 {
            println!();
        }
    }
    println!();
    dbg!(stats.len());
    dbg!(total_data.len());
    if let Some(output_file) = opts.output {
        let mut count_err: HashMap<i64, f64> = HashMap::new();
        let llq = LinearLogQuantizer::new(1.0);

        let f = File::create(output_file).unwrap();
        let mut buffer = std::io::BufWriter::new(f);
        let item_count = 100_000_000;
        let f = ManualFn::new(MN_BASIC, item_count);
        // let huffman = HuffmanI64::new(f.get_huffman_weights(32768, 8000));
        let hmaps = HuffmanMapS::new_unsigned(f);
        let weights = hmaps.get_huffmap();
        let huffman = HuffmanI64::new(weights.into_iter().collect());
        let mut bitbuf = BitVec::new();
        for row in total_data.iter() {
            let mut prev = 0;
            for val in row {
                let nval: i64 = *val - prev;
                let ev = llq.encode(nval);
                let v = llq.decode(ev);
                let counter = count_err.entry(v).or_insert(0.0);
                let bcksz = llq.bucket_size(llq.encode(v));
                *counter += 1.0 / bcksz as f64;

                prev = *val;
                let diff = DiffValue::new_corrected(nval);
                let hkey = hmaps.to_hkey(diff);
                if let Err(e) = huffman.encode(&mut bitbuf, hkey.key) {
                    panic!("Error trying to encode value: {} {:?}", val, e);
                }
                let mut extra_data = hkey.encode_extra();
                bitbuf.append(&mut extra_data);
                // if let Err(e) = huffman.encode(&mut bitbuf, prev) {
                //     panic!("Error trying to encode value: {} {:?}", val, e);
                // }
            }
        }
        let vbuf = bitbuf.to_bytes();
        buffer.write_all(&vbuf).unwrap();
        dbg!(count_err);
        /*
        rmp::encode::write_array_len(&mut buffer, total_data.len() as u32).unwrap();
        let mut tot = 0;
        let mut items = 0;
        let mut max = 0;
        for row in total_data.iter() {
            rmp::encode::write_array_len(&mut buffer, 7).unwrap();
            let mut prev = 0;

            for val in row {
                let nval: i64 = *val - prev;
                prev = *val;
                tot += nval;
                items += 1;
                max = max.max(nval);
                rmp::encode::write_uint(&mut buffer, nval as u64).unwrap();
            }
        }
        dbg!(tot as f64 / items as f64);
        dbg!(max);*/
    }
}

fn read_inputfile(filename: &str, stats: &mut HashMap<i64, f64>) -> Vec<[i64; 7]> {
    let f = File::open(filename).unwrap();
    let mut reader = BufReader::new(f);
    let mut fdv = FrameDataVec::new();
    if fdv.read(&mut reader, 100000).is_err() {
        // dbg!(e);
    }
    // dbg!(fdv.v.len());

    // test_batchdata_compression(fdv.v);
    // test_serializer(fdv.v);
    let lq = LinearLogQuantizer::new(0.001);
    let bd = BatchData::new(fdv.v);
    bd.test_recv_composite_compression(stats);

    bd.recv_us
        .iter()
        .map(|x| -> [i64; 7] {
            [
                lq.encode(x[0] as i64),
                lq.encode(x[1] as i64),
                lq.encode(x[2] as i64),
                lq.encode(x[3] as i64),
                lq.encode(x[4] as i64),
                lq.encode(x[5] as i64),
                lq.encode(x[6] as i64),
            ]
        })
        .collect()
}

#[allow(dead_code)]
fn test_batchdata_compression(v: Vec<FrameData>) {
    let bd = BatchData::new(v);

    let tests: Vec<Box<dyn Compress<f32>>> = vec![
        // Box::new(fft::PolarCompress::default()),
        //Box::new(quantize::LogQuantizer::default()),
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
    let test_vec = &trasposed_recv[1][..600];
    // let test_vec = BatchData::flatten(&bd.recv_us[..1]);

    let mut serializer = quantize::LogQuantizer::default();
    serializer.compress(&test_vec).unwrap();
    let ser_data = serializer.serialize().unwrap();
    dbg!(ser_data.len());
    dbg!(ser_data.len() as f32 * 8.0 / test_vec.len() as f32);

    let mut deserializer = quantize::LogQuantizer::default();
    deserializer.deserialize(&ser_data).unwrap();
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
