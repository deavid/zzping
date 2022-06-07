use std::time::{Duration, SystemTime};

use crate::framedataq::{Complete, FrameDataQ};
use anyhow::{Context, Result};

use thiserror::Error;

#[derive(Debug, Error)]
enum PingError {
    #[error("error parsing data")]
    ParseError,
}

#[derive(Debug, Clone, Copy)]
pub struct Ping {
    pub received: SystemTime,
    pub rtt_us: i64,
}

impl Ping {
    pub fn from_framedataq(frame: FrameDataQ<Complete>) -> Result<Self> {
        let ts = frame
            .timestamp
            .ok_or(PingError::ParseError)
            .context("timestamp parse")? as u64;
        let sub_ms = frame.subsec_ms.try_abs().context("sub_ms parse")? as u64;
        let d = Duration::from_millis(ts * 1000 + sub_ms);
        let received = SystemTime::UNIX_EPOCH
            .checked_add(d)
            .ok_or(PingError::ParseError)
            .context("systemtime add")?;
        let rtt_us = frame.recv_us[3];
        Ok(Self { received, rtt_us })
    }
}

#[derive(Debug, Clone)]
pub struct FirPingConfig {
    /// Initial time for the series (can include data outside of the series)
    pub start: SystemTime,
    /// End time for the series (can include data outside of the series)
    pub end: SystemTime,
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
            start: SystemTime::UNIX_EPOCH,
            end: SystemTime::UNIX_EPOCH,
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
}

impl FirPing {
    /// Converts a list of pings into FIR using the provided config.
    /// Interval is the (intended) frequency of the input pings.
    pub fn from_pings(cfg: FirPingConfig, _interval: Duration, pings: Vec<Ping>) -> Self {
        let mut ret = Self {
            cfg,
            rtt: vec![],
            max_density: 1.0, // FIXME
        };
        ret.load(pings);
        ret
    }
    fn load(&mut self, pings: Vec<Ping>) {
        // use probability::distribution::Continuous;
        use probability::distribution::Distribution;
        let w_size_secs: f64 = self.cfg.window_size.as_secs_f64();
        let interval = self.cfg.interval.as_secs_f64();
        let window = probability::distribution::Gaussian::new(0.0, w_size_secs / interval);
        let sigmas = w_size_secs / interval * self.cfg.sigmas;

        let size_secs = self
            .cfg
            .end
            .duration_since(self.cfg.start)
            .unwrap()
            .as_secs_f64();
        let fir_size = (size_secs / interval).ceil() as usize + 1;
        self.rtt = vec![(0.0, 0.0); fir_size];

        for p in pings {
            let rtt = p.rtt_us as f64 / 1_000_000.0;
            // let rtt2 = (p.rtt_us as f64 / 1_000.0).powi(9);
            let rtt2 = 1.0;
            let d = match p.received.duration_since(self.cfg.start) {
                Ok(v) => v.as_secs_f64(),
                Err(e) => -e.duration().as_secs_f64(),
            };
            let pos = d / interval;
            let l = (pos - sigmas).floor().max(0.0) as usize;
            let r = (pos + sigmas).ceil().min((fir_size - 1) as f64) as usize;
            for n in l..=r {
                let wpos = n as f64 - pos;
                let density = window.distribution(wpos + 0.5) - window.distribution(wpos - 0.5);
                // println!("{},{}", wpos, density);
                let rdens = rtt * rtt2 * density;
                let item = self.rtt.get_mut(n).unwrap();
                item.0 += rdens;
                item.1 += density * rtt2;
            }
        }
        // density indicates pings per interval - that makes sense.
    }
}
