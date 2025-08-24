use crate::chronohelpers::ChronoHelperDuration;
use crate::framedataq::{Complete, FrameDataQ};
use anyhow::{Context, Result};
use chrono::{DateTime, SecondsFormat, Utc};
use std::{
    collections::VecDeque,
    io::{BufRead, BufReader, Read, Write},
    net::IpAddr,
    time::{Duration, Instant},
};
use thiserror::Error;

#[derive(Debug, Error)]
enum PingError {
    #[error("error parsing data")]
    ParseError,
    #[error("Unexpected file line: {0:?}")]
    UnexpectedFileLine(String),
}

#[derive(Debug, Clone, Copy)]
pub struct Ping {
    pub received: DateTime<Utc>,
    pub rtt_us: i64,
}

impl Ping {
    pub fn from_framedataq(frame: FrameDataQ<Complete>) -> Result<Self> {
        let ts = frame
            .timestamp
            .ok_or(PingError::ParseError)
            .context("timestamp parse")? as u64;
        let sub_ms = frame.subsec_ms.try_abs().context("sub_ms parse")? as u64;
        let ts = ts + sub_ms / 1000;
        let sub_ms = sub_ms % 1000;
        let received =
            DateTime::<Utc>::from_timestamp(ts as i64, sub_ms as u32 * 1_000_000).unwrap();
        let rtt_us = frame.recv_us[3];
        Ok(Self { received, rtt_us })
    }
}

#[derive(Debug, Clone)]
pub struct FirPingConfig {
    /// Initial time for the series (can include data outside of the series)
    pub start: DateTime<Utc>,
    /// End time for the series (can include data outside of the series)
    pub end: DateTime<Utc>,
    /// Sampling frequency
    pub interval: Duration,
    /// Width of the window, where nearly 70% of the values fall into.
    pub window_size: Duration,

    /// Defines how long to compute the window, leaving "data" behind
    /// on the low-end of the curve.
    ///
    /// loss: 2-> 4% 3-> 0.2% 4 -> 0.006% (3 sigmas looks exactly the same as 8 sigmas)
    pub sigmas: f64,
}

impl Default for FirPingConfig {
    fn default() -> Self {
        Self {
            start: DateTime::<Utc>::MIN_UTC,
            end: DateTime::<Utc>::MIN_UTC,
            interval: Default::default(),
            window_size: Default::default(),
            sigmas: Default::default(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct FirPing {
    pub cfg: FirPingConfig,
    /// Ping RTT in format of (RTT density, Point Density)
    ///
    /// rtt.0 / rtt.1 gives rtt time - rtt.1 is density
    pub rtt: Vec<(f64, f64)>,

    /// Given sigmas, what is the maximum density expected.
    /// This is used to correct the point density.
    /// FIXME: This does not account for the input point density - meaning that
    /// depending on the frequency of the pings, it might look higher or lower.
    pub max_density: f64,

    pub dct: Dct,
}

impl FirPing {
    /// Converts a list of pings into FIR using the provided config.
    /// Interval is the (intended) frequency of the input pings.
    pub fn from_pings(cfg: &FirPingConfig, _interval: Duration, pings: &[Ping], dev: i32) -> Self {
        let mut ret = Self {
            cfg: cfg.clone(),
            rtt: vec![],
            max_density: 1.0, // FIXME
            dct: Default::default(),
        };
        ret.load(pings, dev);
        ret
    }
    fn load(&mut self, pings: &[Ping], dev: i32) {
        use crate::window::GAUSS_WINDOW;
        use crate::window::SINC_WINDOW;
        let w_size_secs: f64 = self.cfg.window_size.as_secs_f64();
        let interval = self.cfg.interval.as_secs_f64();
        let w_width = w_size_secs / interval;
        let sigmas = w_size_secs / interval * self.cfg.sigmas;

        let size_secs = self
            .cfg
            .end
            .signed_duration_since(self.cfg.start)
            .as_secs_f64();

        let fir_size = (size_secs / interval).ceil() as usize + 1;
        self.rtt = vec![(0.0, 0.0); fir_size];
        let dur: Vec<_> = pings
            .iter()
            .map(|p| {
                p.received
                    .signed_duration_since(self.cfg.start)
                    .as_secs_f64()
            })
            .collect();

        for (n, p) in pings.iter().enumerate() {
            let rtt = p.rtt_us as f64 / 1_000_000.0;
            let rtt2 = (p.rtt_us as f64 / 10_000.0).powi(dev);
            let d = dur[n];
            let pos = d / interval;
            let prev_pos = dur[(n.saturating_sub(2)).clamp(0, dur.len() - 1)] / interval;
            let next_pos = dur[(n + 2).clamp(0, dur.len() - 1)] / interval;
            // Scale factor finds gaps (packet loss) and ensures they have data in between (i.e. no NaN)
            let scale_factor_r = ((next_pos - pos) / 2.0).max(1.0);
            let scale_factor_l = ((pos - prev_pos) / 2.0).max(1.0);
            let scale_factor = (scale_factor_l + scale_factor_r) / 2.0;
            let l = (pos - sigmas * scale_factor_l).floor().max(0.0) as usize;
            let r = (pos + sigmas * scale_factor_r)
                .ceil()
                .min((fir_size - 1) as f64) as usize;
            let wpos = l as f64 - pos;

            let mut prev_density_gauss = GAUSS_WINDOW.get(wpos - 0.5, w_width);
            let mut prev_density_sinc = SINC_WINDOW.get(wpos - 0.5, w_width);
            let mut prev_density_gauss2 = GAUSS_WINDOW.get(wpos - 0.5, w_width * scale_factor);

            for n in l..=r {
                let wpos = n as f64 - pos;
                let next_density_gauss = GAUSS_WINDOW.get(wpos + 0.5, w_width);
                let next_density_sinc = SINC_WINDOW.get(wpos + 0.5, w_width);
                let next_density_gauss2 = GAUSS_WINDOW.get(wpos + 0.5, w_width * scale_factor);

                // dbg!(wpos);
                let density_gauss = next_density_gauss - prev_density_gauss;
                let density_sinc = next_density_sinc - prev_density_sinc;
                let density_gauss2 = next_density_gauss2 - prev_density_gauss2;

                prev_density_gauss = next_density_gauss;
                prev_density_gauss2 = next_density_gauss2;
                prev_density_sinc = next_density_sinc;

                let density = density_sinc * density_gauss + density_gauss2 * 0.01;
                // x.powf is extremely slow!
                // let density = density.powf(dev.abs() as f64 + 1.0);
                // println!("{},{}", wpos, density);
                let rdens = rtt * rtt2 * density;
                let item = self.rtt.get_mut(n).unwrap();
                item.0 += rdens;
                item.1 += density * rtt2;
            }
        }
        // density indicates pings per interval - that makes sense.
        // TODO: optimally, we should prevent here leaving with density = 0. Extra smoothing from neighbors would be nice.
        self.dct = Dct::from_rd(&self.rtt);
    }
    pub fn dct_test(&self) {
        use rustdct::DctPlanner;
        let k: f64 = 2.0 / self.rtt.len() as f64;
        let buffer: Vec<_> = self.rtt.iter().map(|(r, d)| r / d).collect();
        println!("orig: {:?}", &buffer[..64]);
        let mut buffer: Vec<_> = self.rtt.iter().map(|(r, d)| r / d * k).collect();
        let t = Instant::now();
        let mut planner = DctPlanner::new();
        let dct2 = planner.plan_dct2(self.rtt.len());
        dct2.process_dct2(&mut buffer);
        println!("dct2: {:?}", &buffer[..1024]);
        println!("forward: {:?}", t.elapsed());
        let t = Instant::now();

        let len = 100;
        let dct3 = planner.plan_dct3(len);
        dct3.process_dct3(&mut buffer[..len]);
        // for n in buffer.iter_mut() {
        //     *n *= k;
        // }
        println!("dct3: {:?}", &buffer[..64]);
        println!("inverse: {:?}", t.elapsed());

        //
    }
}

#[derive(Debug, Clone, Default)]
pub struct Dct {
    pub orig_data: Vec<f64>,
    pub data: Vec<f64>,
}

impl Dct {
    pub fn from_rd(rd: &[(f64, f64)]) -> Self {
        use rustdct::DctPlanner;
        let mut planner = DctPlanner::new();

        let len = rd.len();
        let k: f64 = 2.0 / len as f64;
        let orig_data: Vec<f64> = rd.iter().map(|(r, d)| r / d).collect();
        let mut data: Vec<f64> = orig_data.iter().map(|x| x * k).collect();
        let dct2 = planner.plan_dct2(len);
        dct2.process_dct2(&mut data);

        Self { data, orig_data }
    }
    pub fn export(&self, len: usize, loss: f64) -> Vec<f64> {
        use rustdct::DctPlanner;
        let mut planner = DctPlanner::new();
        let len2 = len.min(self.data.len());
        // if len != len2 {
        //     warn!("Dct::export({}) - max len is {}", len, len2);
        // }
        let mut buffer: Vec<f64> = self.data[..len2].to_vec();
        let skip = len2 / 16;
        let blck_sz = 10000;
        let mut range: Vec<f64> = vec![1.0e-60; len2 / blck_sz + 1];
        for (n, x) in buffer.iter().skip(skip).copied().enumerate() {
            let x = x.abs();
            let n = n / blck_sz;
            range[n] = range[n].max(x);
        }

        let loss_v: Vec<f64> = range.into_iter().map(|x| loss * x).collect();
        // let mut rnd = rand::thread_rng();
        if loss > 0.0 {
            // let dbg: Vec<_> = buffer
            //     .iter()
            //     .skip(skip)
            //     .enumerate()
            //     .map(|(n, x)| ((*x / loss_v[n / blck_sz]).abs().round() * x.signum()) as i64)
            //     .collect();

            buffer.iter_mut().skip(skip).enumerate().for_each(|(n, x)| {
                *x = (*x / loss_v[n / blck_sz]).abs().round() * x.signum() * loss_v[n / blck_sz]
            });
            // println!("dct: {:?}", dbg);
        }

        // if len > len2 {
        //     let mut zeros = vec![0.0; len.min(len2 * 2) - len2];
        //     buffer.append(&mut zeros);
        // }
        let len2 = buffer.len();
        let dct3 = planner.plan_dct3(len2);
        // if len / 2 < len2 {
        //     for (n, p) in buffer[len / 2..len2].iter_mut().enumerate() {
        //         let k = 1.0 - (n as f64 / (len2 / 2) as f64);
        //         *p *= k.powi(6);
        //     }
        // }
        dct3.process_dct3(&mut buffer);
        if buffer.len() == self.data.len() {
            let mut diff = 0.0;
            for (x, y) in buffer.iter().zip(self.orig_data.iter()) {
                diff += (x - y).abs();
            }
            diff /= buffer.len() as f64;
            diff /= self.data[0] / 2.0;
            dbg!(diff * 100.0);
        }
        buffer
    }

    pub fn from_dct(d: &[f64]) -> Self {
        use rustdct::DctPlanner;
        let mut planner = DctPlanner::new();
        let mut buffer = d.to_owned();
        let len = d.len();
        let dct3 = planner.plan_dct3(len);
        dct3.process_dct3(&mut buffer);
        Self {
            orig_data: buffer,
            data: d.to_owned(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct StreamData {
    /// Target Host address.
    pub addr: IpAddr,
    /// How fast we're trying to ping.
    pub send_interval: Duration,
    /// Starting time.
    pub initial_time: chrono::DateTime<chrono::Utc>,
    /// Quantity and amount of microseconds from initial_time that the pings were sent.
    pub pending_pings_us: Vec<u64>,

    /// Used to compute the deltas to the next packet
    pub time_cursor: Instant,
}

impl StreamData {
    pub fn from_file_header<R: Read>(r: &mut BufReader<R>) -> Result<Self> {
        let mut buf = String::new();
        // HEADER
        r.read_line(&mut buf)?;
        if buf.trim() != "StreamData1!" {
            Err(PingError::UnexpectedFileLine(buf.to_owned())).context("invalid header")?
        }
        // IPAddress
        buf.clear();
        r.read_line(&mut buf)?;
        let left = "addr: ";
        if &buf[0..left.len()] != left {
            Err(PingError::UnexpectedFileLine(buf.to_owned())).context("invalid addr field")?
        }
        let addr: IpAddr = buf[left.len()..buf.len()]
            .trim()
            .parse()
            .with_context(|| format!("addr value from: {:?}", buf))?;

        // Send Interval micros
        buf.clear();
        r.read_line(&mut buf)?;
        let left = "send_interval_us: ";
        if &buf[0..left.len()] != left {
            Err(PingError::UnexpectedFileLine(buf.to_owned()))
                .context("invalid send_interval_us field")?
        }
        let send_interval_us: u64 = buf[left.len()..buf.len()]
            .trim()
            .parse()
            .with_context(|| format!("send_interval_us value from: {:?}", buf))?;
        let send_interval = Duration::from_micros(send_interval_us);

        // Initial Time
        buf.clear();
        r.read_line(&mut buf)?;
        let left = "initial_time: ";
        if &buf[0..left.len()] != left {
            Err(PingError::UnexpectedFileLine(buf.to_owned()))
                .context("invalid initial_time field")?
        }
        let initial_time: DateTime<Utc> =
            DateTime::parse_from_rfc3339(buf[left.len()..buf.len()].trim().trim_matches('"'))
                .with_context(|| format!("initial_time value from: {:?}", buf))?
                .into();

        // Pending pings
        buf.clear();
        r.read_line(&mut buf)?;
        let left = "pending_pings_us: ";
        if &buf[0..left.len()] != left {
            Err(PingError::UnexpectedFileLine(buf.to_owned()))
                .context("invalid pending_pings_us field")?
        }
        let pending_pings: String = buf[left.len()..buf.len()].trim().to_owned();
        let mut pending_pings_us: Vec<u64> = vec![];
        for p in pending_pings
            .trim_matches(|c| c == '[' || c == ']')
            .split(',')
        {
            let p = p.trim();
            if p.is_empty() {
                continue;
            }
            let p: u64 = p.parse().context("pending_pings_us")?;
            pending_pings_us.push(p);
        }
        pending_pings_us.sort_by(|a, b| b.cmp(a));
        // END HEADER
        buf.clear();
        r.read_line(&mut buf)?;
        if buf.trim() != "---" {
            Err(PingError::UnexpectedFileLine(buf.to_owned())).context("invalid end header")?
        }
        Ok(Self {
            addr,
            send_interval,
            initial_time,
            pending_pings_us: vec![],    // TODO: parse this
            time_cursor: Instant::now(), // TODO: not useful when decoding!,
        })
    }

    pub fn write_header<W: Write>(&self, mut w: W) -> Result<()> {
        writeln!(w, "StreamData1!")?;
        writeln!(w, "addr: {}", self.addr)?;
        writeln!(w, "send_interval_us: {}", self.send_interval.as_micros())?;
        writeln!(
            w,
            "initial_time: {}",
            self.initial_time
                .to_rfc3339_opts(SecondsFormat::Micros, true),
        )?;
        writeln!(w, "pending_pings_us: {:?}", self.pending_pings_us)?;
        writeln!(w, "---")?;
        // TODO: this header is going to be hell to parse.

        Ok(())
    }

    pub fn write_ping<W: Write>(&mut self, w: W, event: StreamEventType, scode: u8) -> Result<()> {
        let delta = self.time_cursor.elapsed().as_micros() as u64;
        self.time_cursor += Duration::from_micros(delta);
        let sp = StreamPing {
            event,
            delta,
            scode,
        };
        sp.write(w)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEventType {
    // Ping was sent.
    Sent,
    // Ping was received.
    Received,
}

#[derive(Debug, Clone)]
pub struct StreamPing {
    pub event: StreamEventType,
    pub delta: u64,
    pub scode: u8,
}

impl StreamPing {
    const SKIP_TIME_USEC: u64 = 1 << 22;

    pub fn from_reader<R: Read>(mut r: R) -> Result<Self> {
        let mut delta: u64 = 0;
        let mut buf = [0u8; 4];
        r.read_exact(&mut buf).context("StreamPing:r.read_exact")?;
        let mut code = i32::from_be_bytes(buf);
        while code == 0 {
            delta += Self::SKIP_TIME_USEC;
            r.read_exact(&mut buf)?;
            code = i32::from_be_bytes(buf);
        }
        let event = match code > 0 {
            true => StreamEventType::Sent,
            false => StreamEventType::Received,
        };
        let code = code.abs();
        let scode = (code % 256) as u8;
        let code = code / 256 - 1;
        delta += code as u64;
        Ok(Self {
            event,
            delta,
            scode,
        })
    }
    pub fn write<W: Write>(&self, mut w: W) -> Result<()> {
        let t = self.delta;
        let t = self.write_skips(&mut w, t)? as i32 + 1;
        let mut t = t * 256;
        t += self.scode as i32;
        let time: i32 = match self.event {
            StreamEventType::Sent => t,
            StreamEventType::Received => -t,
        };
        let code = time.to_be_bytes();
        w.write_all(&code)?;
        Ok(())
    }

    fn write_skips<W: Write>(&self, mut w: W, mut t: u64) -> Result<u64> {
        let skip: i32 = 0;
        let skipcode = skip.to_be_bytes();
        while t > Self::SKIP_TIME_USEC {
            w.write_all(&skipcode)?;
            t -= Self::SKIP_TIME_USEC;
        }
        Ok(t)
    }
}

#[derive(Debug, Clone)]
pub enum SIOEvent {
    // Ping was sent.
    Sent,
    // Ping was received.
    Received,
    // Ping was lost.
    Lost,
}

#[derive(Debug, Clone)]
pub struct StreamPingIO {
    pub header: StreamData,
    pub delta: u64,
    pub queue: VecDeque<u16>,
}

impl StreamPingIO {
    pub fn from_header(header: StreamData) -> Self {
        let delta = 0;
        // FIXME: Move delta logic in here.
        Self {
            header,
            delta,
            queue: VecDeque::new(),
        }
    }
    pub fn from_file_header<R: Read>(r: &mut BufReader<R>) -> Result<Self> {
        let header = StreamData::from_file_header(r)?;
        Ok(Self::from_header(header))
    }
    pub fn read_next<R: Read>(&mut self, r: R) -> Result<Option<StreamPing>> {
        let mut sp = match StreamPing::from_reader(r) {
            Ok(v) => v,
            Err(e) => {
                if let Some(e) = e.downcast_ref::<std::io::Error>()
                    && e.kind() == std::io::ErrorKind::UnexpectedEof
                {
                    return Ok(None);
                }
                Err(e)?
            }
        };
        self.delta += sp.delta;
        sp.delta = self.delta;
        Ok(Some(sp))
    }
    pub fn write_ping<W: Write>(&mut self, mut w: W, ev: SIOEvent, seqn: u16) -> Result<()> {
        match ev {
            SIOEvent::Sent => {
                if self.queue.len() > 200 {
                    // Signal packet lost to prevent queue overflow
                    self.header.write_ping(&mut w, StreamEventType::Sent, 255)?;
                    self.queue.pop_front();
                }
                self.queue.push_back(seqn);
                self.header.write_ping(w, StreamEventType::Sent, 0)?;
            }
            SIOEvent::Received => {
                let scode = self
                    .queue
                    .iter()
                    .copied()
                    .enumerate()
                    .find(|(_, x)| *x == seqn)
                    .map(|(n, _)| n)
                    .unwrap_or(255) as u8;
                self.queue.remove(scode as usize);

                self.header
                    .write_ping(w, StreamEventType::Received, scode)?;
            }
            SIOEvent::Lost => {
                if let Some(scode) = self
                    .queue
                    .iter()
                    .copied()
                    .enumerate()
                    .find(|(_, x)| *x == seqn)
                    .map(|(n, _)| n as u8)
                {
                    self.header
                        .write_ping(w, StreamEventType::Sent, scode + 1)?;
                    self.queue.remove(scode as usize);
                }
            }
        }

        Ok(())
    }
}
