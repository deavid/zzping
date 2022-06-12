use std::time::{Duration, SystemTime};

use iced::{
    canvas::{self, Frame, Program, Stroke},
    Color, Point,
};
use log::info;
use zzping_lib::pingdata::{FirPing, FirPingConfig, Ping};

use crate::firtest::Msg;

#[derive(Debug)]
pub struct Graph {
    pub firdata: FirPing,
    pub firdata2: FirPing,
    pub firdata3: FirPing,
    pub data: Vec<Ping>,
    pub left: SystemTime,
    pub right: SystemTime,
    pub top: i64,
    pub bottom: i64,
    pub init_len: usize,
    pub first_item: usize,
    pub last_item: usize,
}

impl Graph {
    pub fn load(&mut self, data: Vec<Ping>) {
        self.data = data;
        // .. .calc left, right, etc.
        self.left = SystemTime::now();
        self.right = SystemTime::UNIX_EPOCH;
        self.top = 0;
        self.bottom = 1 << 60;
        self.first_item = 0;

        for (n, p) in self.data.iter().take(7300).enumerate().skip(7100) {
            //for (n, p) in self.data.iter().take(10000).enumerate() {
            let t = p.received;
            let r = p.rtt_us;
            self.left = self.left.min(t);
            self.right = self.right.max(t);
            self.top = self.top.max(r);
            self.bottom = self.bottom.min(r);
            self.last_item = n;
        }
        let k = 5;
        let msec = 10;
        // let msec = 60 * 1000;
        // let cfg = FirPingConfig {
        //     start: self.left,
        //     end: self.right,
        //     interval: Duration::from_millis(msec / 2),
        //     window_size: Duration::from_millis(msec),
        //     sigmas: 3.0,
        // };
        // self.firdata = FirPing::from_pings(cfg, Duration::from_millis(10), &self.data, 0);
        // let cfg = FirPingConfig {
        //     start: self.left,
        //     end: self.right,
        //     interval: Duration::from_millis(msec * k / 2),
        //     window_size: Duration::from_millis(msec * k),
        //     sigmas: 3.0,
        // };
        // self.firdata2 = FirPing::from_pings(cfg, Duration::from_millis(10), &self.data, 0);
        let cfg = FirPingConfig {
            start: self.left,
            end: self.right,
            interval: Duration::from_millis(msec),
            window_size: Duration::from_millis(msec),
            sigmas: 8.0,
        };
        let w = 7;
        self.firdata3 = FirPing::from_pings(cfg.clone(), Duration::from_millis(10), &self.data, 0);
        let cfg = FirPingConfig {
            start: self.left,
            end: self.right,
            interval: Duration::from_millis(msec),
            window_size: Duration::from_millis(msec * 4),
            sigmas: 8.0,
        };
        self.firdata2 = FirPing::from_pings(cfg.clone(), Duration::from_millis(10), &self.data, w);
        self.firdata = FirPing::from_pings(cfg, Duration::from_millis(10), &self.data, -w);
        // let firtop: f64 = self
        //     .firdata2
        //     .rtt
        //     .iter()
        //     .copied()
        //     .filter_map(|(r, d)| match d > 0.0 {
        //         true => Some(r / d),
        //         false => None,
        //     })
        //     .fold(0.0, |acc, x| x.max(acc))
        //     * 1.1;
        // self.top = self.top.min((firtop * 1_000_000.0) as i64);

        // let firbottom: f64 = self
        //     .firdata
        //     .rtt
        //     .iter()
        //     .copied()
        //     .filter_map(|(r, d)| match d > 0.0 {
        //         true => Some(r / d),
        //         false => None,
        //     })
        //     .fold(f64::INFINITY, |acc, x| x.min(acc))
        //     / 1.1;
        // self.bottom = self.bottom.max((firbottom * 1_000_000.0) as i64);
        if self.bottom == self.top {
            self.bottom -= 1;
            self.bottom /= 2;
        }
        // for w in self.firdata.rtt.windows(2) {
        //     const ALMOST_ZERO: f64 = 0.000001;
        //     let l = w[0];
        //     let r = w[1];
        //     if l.1 < ALMOST_ZERO || r.1 < ALMOST_ZERO {
        //         continue;
        //     }
        //     let rttl = l.0 / l.1;
        //     let rttr = r.0 / r.1;
        //     println!("{}", (rttl / rttr).log2());
        // }
        dbg!(self.firdata3.rtt.len());
    }
    pub fn to_screen(&self, sz: iced::Size, v: &Ping) -> Point {
        let d = match v.received.duration_since(self.left) {
            Ok(v) => v.as_secs_f32(),
            Err(e) => -e.duration().as_secs_f32(),
        };
        let rd = self.right.duration_since(self.left).unwrap().as_secs_f32();
        let x: f32 = d / rd * sz.width;

        let y: f32 = sz.height
            - (v.rtt_us - self.bottom) as f32 / (self.top - self.bottom) as f32 * sz.height;
        if !x.is_finite() {
            panic!("to_screen - x is {:?}", x);
        }
        if !y.is_finite() {
            panic!(
                "to_screen - y is {:?} - top:{:?} - bottom:{}",
                y, self.top, self.bottom
            );
        }
        Point::new(x, y)
    }
}

impl Program<Msg> for &mut Graph {
    fn draw(
        &self,
        bounds: iced::Rectangle,
        _cursor: iced::canvas::Cursor,
    ) -> Vec<iced::canvas::Geometry> {
        let sz = bounds.size();
        let mut frame = Frame::new(sz);
        let green_st = Stroke {
            width: 1.5,
            color: Color::from_rgba8(0, 200, 0, 0.8),
            ..Stroke::default()
        };
        let yellow_st = Stroke {
            width: 1.5,
            color: Color::from_rgba8(255, 192, 0, 0.5),
            ..Stroke::default()
        };
        let orange_st = Stroke {
            width: 1.5,
            color: Color::from_rgba8(255, 64, 0, 0.6),
            ..Stroke::default()
        };
        let black_st = Stroke {
            width: 1.5,
            color: Color::from_rgba8(0, 0, 0, 0.9),
            ..Stroke::default()
        };
        for w in self
            .data
            .windows(2)
            .skip(self.first_item)
            .take(self.last_item - self.first_item)
            .take(10000000)
        {
            let l = w[0];
            let r = w[1];
            frame.stroke(
                &canvas::Path::line(self.to_screen(sz, &l), self.to_screen(sz, &r)),
                green_st,
            );
        }

        const ALMOST_ZERO: f64 = 0.00000000000000001;
        let mut posl = self.firdata.cfg.start;
        for w in self.firdata.rtt.windows(2).take(100000) {
            let l = w[0];
            let r = w[1];
            if l.1 < ALMOST_ZERO || r.1 < ALMOST_ZERO {
                posl += self.firdata.cfg.interval;
                continue;
            }
            let rttl = l.0 * 1_000_000.0 / l.1;
            let rttr = r.0 * 1_000_000.0 / r.1;
            let posr = posl + self.firdata.cfg.interval;
            let l = Ping {
                received: posl,
                rtt_us: rttl.round() as i64,
            };
            let r = Ping {
                received: posr,
                rtt_us: rttr.round() as i64,
            };
            frame.stroke(
                &canvas::Path::line(self.to_screen(sz, &l), self.to_screen(sz, &r)),
                yellow_st,
            );
            posl += self.firdata.cfg.interval;
        }
        let mut posl = self.firdata2.cfg.start;
        for w in self.firdata2.rtt.windows(2).take(100000) {
            let l = w[0];
            let r = w[1];
            if l.1 < ALMOST_ZERO || r.1 < ALMOST_ZERO {
                posl += self.firdata2.cfg.interval;
                continue;
            }
            let rttl = l.0 * 1_000_000.0 / l.1;
            let rttr = r.0 * 1_000_000.0 / r.1;
            let posr = posl + self.firdata2.cfg.interval;
            let l = Ping {
                received: posl,
                rtt_us: rttl.round() as i64,
            };
            let r = Ping {
                received: posr,
                rtt_us: rttr.round() as i64,
            };
            frame.stroke(
                &canvas::Path::line(self.to_screen(sz, &l), self.to_screen(sz, &r)),
                orange_st,
            );
            posl += self.firdata2.cfg.interval;
        }
        let mut posl = self.firdata3.cfg.start;
        for w in self.firdata3.rtt.windows(2).take(100000) {
            let l = w[0];
            let r = w[1];
            if l.1 < ALMOST_ZERO || r.1 < ALMOST_ZERO {
                posl += self.firdata3.cfg.interval;
                continue;
            }
            let rttl = l.0 * 1_000_000.0 / l.1;
            let rttr = r.0 * 1_000_000.0 / r.1;
            let posr = posl + self.firdata3.cfg.interval;
            let l = Ping {
                received: posl,
                rtt_us: rttl.round() as i64,
            };
            let r = Ping {
                received: posr,
                rtt_us: rttr.round() as i64,
            };
            frame.stroke(
                &canvas::Path::line(self.to_screen(sz, &l), self.to_screen(sz, &r)),
                black_st,
            );
            posl += self.firdata3.cfg.interval;
        }
        vec![frame.into_geometry()]
    }
}

impl Default for Graph {
    fn default() -> Self {
        Self {
            left: SystemTime::UNIX_EPOCH,
            right: SystemTime::now(),
            data: Default::default(),
            top: Default::default(),
            bottom: Default::default(),
            init_len: Default::default(),
            first_item: Default::default(),
            last_item: Default::default(),
            firdata: Default::default(),
            firdata2: Default::default(),
            firdata3: Default::default(),
        }
    }
}
