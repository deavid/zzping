use std::{
    collections::VecDeque,
    fs::File,
    io::{BufReader, BufWriter, Write},
    time::Duration,
};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use clap::Parser;
use log::{error, info, warn};
use zzping_lib::pingdata::{Dct, FirPing, FirPingConfig, Ping, StreamEventType, StreamPingIO};

/// Converts StreamData format into DCT/FIR data format.
#[derive(Parser, Debug)]
#[clap(name = "stream2dct", version, about)]
struct Opts {
    #[clap(short, long)]
    input: String,
    #[clap(short, long)]
    output: String,
}

#[derive(Debug)]
pub struct StreamDataFile {
    pub spr: StreamPingIO,
    pub ping: Vec<Ping>,
    pub loss: Vec<Ping>,
}

impl StreamDataFile {
    fn load(mut buf: BufReader<File>) -> Result<Self> {
        let mut data: Vec<Ping> = vec![];
        let mut loss: Vec<Ping> = vec![];
        let mut spr = StreamPingIO::from_file_header(&mut buf).context("SD:header")?;
        let max_ping = spr
            .header
            .pending_pings_us
            .first()
            .copied()
            .unwrap_or_default();
        let mut pending: VecDeque<u64> = spr
            .header
            .pending_pings_us
            .iter()
            .copied()
            .map(|x| x - max_ping)
            .collect();

        let init_time: DateTime<Utc> = spr.header.initial_time;
        let mut cur_time;
        while let Some(sp) = spr.read_next(&mut buf).context("SDR:read")? {
            cur_time = init_time + chrono::Duration::microseconds(sp.delta as i64);
            match sp.event {
                StreamEventType::Received => {
                    if pending.is_empty() {
                        warn!("{}us - no ping sent?", sp.delta);
                        continue;
                    }
                    let idx = sp.scode.min(pending.len() as u8 - 1);
                    if idx != sp.scode {
                        warn!(
                            "ping sent code: {} but queue has size {}",
                            sp.scode,
                            pending.len()
                        );
                    }
                    let sent = pending.remove(idx as usize).unwrap();
                    let rtt_us = (sp.delta - sent) as i64;
                    // if sp.scode != 0 {
                    //     info!("{}us - {} left code:{}", rtt_us, pending.len(), sp.scode);
                    // }
                    let sent_time = init_time + chrono::Duration::microseconds(sent as i64);

                    data.push(Ping {
                        // Reporting from sent_time instead from receiving time seems to be better.
                        // The problem is that this metric might be "in the past" of a block.
                        received: sent_time,
                        rtt_us,
                    });
                }
                StreamEventType::Sent => {
                    if sp.scode > 0 {
                        if pending.is_empty() {
                            warn!("packet lost: {} - queue is empty", sp.scode);
                            continue;
                        }
                        let idx = if sp.scode == 255 { 0 } else { sp.scode - 1 };
                        let idx = idx.min(pending.len() as u8 - 1);
                        let sent = pending.remove(idx as usize).unwrap();
                        let sent_time = init_time + chrono::Duration::microseconds(sent as i64);
                        for p in loss.iter_mut().rev() {
                            if p.received <= sent_time && p.rtt_us > 500_000 {
                                p.rtt_us = 500_000;
                                break;
                            }
                        }
                    } else {
                        pending.push_back(sp.delta);
                        loss.push(Ping {
                            received: cur_time,
                            rtt_us: 1_000_000,
                        });
                    }
                }
            }
        }
        Ok(Self {
            spr,
            ping: data,
            loss,
        })
    }

    fn to_fir(&self) -> FirPing {
        let usec = self.spr.header.send_interval.as_micros() as u64;
        let interval = Duration::from_micros(usec);
        // Multiplier for the window size respective to ping interval
        let wsize_mult = 1.0;
        // How many samples to look left and right (min_q:3.0 max_q: 6.0)
        let sigmas = 6.0;
        let w0 = 0;

        let cfg = FirPingConfig {
            start: self.spr.header.initial_time,
            end: self.spr.header.initial_time
                + chrono::Duration::microseconds((self.spr.delta + usec) as i64),
            interval,
            window_size: Duration::from_micros((usec as f64 * wsize_mult).round() as u64),
            sigmas,
        };
        FirPing::from_pings(&cfg, interval, &self.ping, w0)
    }
}

struct DctWriter {
    fir: FirPing,
}

impl DctWriter {
    fn from_fir(fir: FirPing) -> Self {
        Self { fir }
    }
    fn write_all(&self, out: &mut BufWriter<File>) -> Result<()> {
        const CHUNK_SIZE: usize = 8192;
        let mut stddev = 0.0;
        let mut count = 0.0;
        for chunk in self.fir.rtt.chunks(CHUNK_SIZE) {
            let dct = Dct::from_rd(chunk);
            stddev += self.write(out, &dct)?;
            count += 1.0;
        }
        stddev /= count;
        info!("normalized stddev: {:.3}%", stddev * 100.0);

        Ok(())
    }
    fn write(&self, out: &mut BufWriter<File>, dct: &Dct) -> Result<f64> {
        let data = &dct.data;
        let data0 = data[0];
        let data1 = &data[1..];
        let mut dct_out: Vec<f64> = vec![data0];
        let mut dct_validation = dct_out.clone();
        out.write_all(&data0.to_be_bytes())?;

        let dct_stddev = Self::stddev_zero(data1);

        let k: f64 = 10.0;
        let rfix = dct_stddev / 5.0;
        out.write_all(&k.to_be_bytes())?;
        out.write_all(&rfix.to_be_bytes())?;

        let d: Vec<f64> = data1
            .iter()
            .map(|v| Self::linpow_forward(*v, rfix, k))
            .collect();
        let min_val = d.iter().copied().fold(f64::MAX, f64::min);
        let max_val = d.iter().copied().fold(f64::MIN, f64::max);
        let range = max_val - min_val;

        out.write_all(&range.to_be_bytes())?;
        out.write_all(&min_val.to_be_bytes())?;

        let len = d.len() as u32;
        out.write_all(&len.to_be_bytes())?;

        for val in d.iter().copied() {
            let v = Self::normalize_u8_forward(val, min_val, range);
            let vr = v.round() as u8;
            out.write_all(&vr.to_be_bytes())?;

            let j = Self::normalize_u8_backward(vr as f64, min_val, range);
            let j = Self::linpow_backward(j, rfix, k);

            dct_out.push(j);

            let j = Self::normalize_u8_backward(v, min_val, range);
            let j = Self::linpow_backward(j, rfix, k);
            dct_validation.push(j);
        }
        let dct_out = Dct::from_dct(&dct_out);
        let stddev_out = Self::stddev_norm_slices(&dct_out.orig_data, &dct.orig_data);

        let dct_validation = Dct::from_dct(&dct_validation);
        let stddev_validation = Self::stddev_norm_slices(&dct_validation.orig_data, &dct.orig_data);
        if stddev_validation > 0.0001 {
            error!(
                "validation stddev is too high! probably a math error has ocurred: {:.6}%",
                stddev_validation * 100.0
            );
        }
        Ok(stddev_out)
    }

    /// Computes the standard deviation  over 0.0 for a list of points.
    fn stddev_zero(data: &[f64]) -> f64 {
        let dct_variance: f64 = data.iter().map(|p| p.powi(2)).sum::<f64>() / data.len() as f64;
        dct_variance.sqrt()
    }

    /// Transforms the points to give better resolution on the close to zero numbers
    /// while losing resolution on the higher ones. `k` controls the power factor
    /// while `rfix` controls the linear part to avoid over-amplifying small details.
    fn linpow_forward(v: f64, rfix: f64, k: f64) -> f64 {
        ((v.abs() + rfix).powf(k.recip()) - rfix.powf(k.recip())) * v.signum()
    }

    /// Undoes the changes made by linpow_forward.
    fn linpow_backward(j: f64, rfix: f64, k: f64) -> f64 {
        ((j.abs() + rfix.powf(k.recip())).powf(k) - rfix) * j.signum()
    }

    /// Normalizes the datapoints to cover all 256 values of a u8.
    fn normalize_u8_forward(val: f64, min_val: f64, range: f64) -> f64 {
        let v = (val - min_val) / range;
        v * (u8::MAX) as f64
    }

    /// Undoes the normalization step from normalize_u8_forward.
    fn normalize_u8_backward(val: f64, min_val: f64, range: f64) -> f64 {
        let j = val as f64 / (u8::MAX) as f64;
        j * range + min_val
    }

    /// Returns the normalized standard deviation between two slices expressed
    /// in factor form, where `1.0` means 100% deviation from mean and `0.0`
    /// means no deviation.
    fn stddev_norm_slices(a_slice: &[f64], b_slice: &[f64]) -> f64 {
        assert_eq!(a_slice.len(), b_slice.len());
        let l = a_slice.len() as f64;
        let mut sum = 0.0;
        let mut dev = 0.0;

        for (a, b) in a_slice.iter().zip(b_slice.iter()) {
            let diff = (a - b).abs();
            let mid = (a + b) / 2.0;
            sum += mid;
            dev += diff.powi(2);
        }
        let variance = dev / l;
        let stddev = variance.sqrt();
        let mean = sum / l;
        stddev / mean
    }
}

fn main() -> Result<()> {
    use env_logger::Env;
    env_logger::Builder::from_env(Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .filter_module("wgpu_core", log::LevelFilter::Error)
        .filter_module("wgpu_hal", log::LevelFilter::Error)
        .init();

    let opts: Opts = Opts::parse();
    let f = File::open(opts.input).context("opening input for read")?;
    let buf = BufReader::new(f);
    info!("loading data...");
    let data = StreamDataFile::load(buf).context("input data load")?;
    info!("done.");
    // dbg!(&data.spr.header);
    // dbg!(&data.ping[0..2]);

    info!("converting to FIR data...");
    let fir = data.to_fir();
    info!("done.");
    // dbg!(&fir.rtt[0..2]);
    // dbg!(&fir.dct.data[0..20]);

    let mut out = std::io::BufWriter::new(File::create(opts.output)?);
    let dctw = DctWriter::from_fir(fir);
    dctw.write_all(&mut out)?;

    Ok(())
}
