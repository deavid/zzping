use chrono::{DateTime, Utc};
use iced::{
    widget::canvas::{self, Frame, Stroke},
    Point, Size,
};
use zzping_lib::{
    chronohelpers::{ChronoHelperDatetime, ChronoHelperDuration},
    pingdata::{FirPing, Ping},
};

#[derive(Debug, Clone)]
pub struct Viewport {
    pub left_time: DateTime<Utc>,
    pub right_time: DateTime<Utc>,
    pub top_pos: f64,
    pub bottom_pos: f64,
    pub zoom: f64,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            left_time: DateTime::<Utc>::now(),
            right_time: DateTime::<Utc>::unix_epoch(),
            top_pos: (1_u64 >> 60) as f64,
            bottom_pos: 0.0,
            zoom: 1.0,
        }
    }
}

impl Viewport {
    pub fn extend(&mut self, t: DateTime<Utc>, y: f64) {
        self.left_time = self.left_time.min(t);
        self.right_time = self.right_time.max(t);
        self.top_pos = self.top_pos.max(y);
        self.bottom_pos = self.bottom_pos.min(y);
        if self.left_time == self.right_time {
            self.right_time += chrono::Duration::nanoseconds(1);
        }
        if self.top_pos == self.bottom_pos {
            self.top_pos += 0.000000001;
        }
    }
    #[allow(dead_code)]
    pub fn zoom(&self, z: f64) -> Self {
        let mut r = self.clone();
        let w = (self.right_time - self.left_time).as_secs_f64() / z.max(1.0);
        let new_r = self.left_time + chrono::Duration::microseconds((w * 1_000_000.0) as i64);
        r.right_time = new_r;
        r
    }
}

#[derive(Debug, Clone, Default)]
pub struct PointGraph {
    // Points X position are always scaled from 0..1
    pub points: Vec<Point2D>,
}

impl PointGraph {
    pub fn draw(&self, frame: &mut Frame, stroke: Stroke, vw: &Viewport) {
        let sz = frame.size();
        let points_per_px = (self.points.len() as f32 / sz.width / 400.0 / vw.zoom as f32)
            .floor()
            .max(1.0) as usize;
        let count = (self.points.len() as f64 / vw.zoom).ceil() as usize;
        for w in self.points[0..count]
            .windows(points_per_px + 1)
            .step_by(points_per_px)
        {
            let mut l = w[0];
            let mut r = w[points_per_px];
            if points_per_px > 1 {
                let valid: Vec<_> = w.iter().copied().filter(Point2D::is_valid).collect();
                if valid.is_empty() {
                    continue;
                }
                let min_x = valid.iter().map(|p| p.x).reduce(f64::min).unwrap();
                let max_x = valid.iter().map(|p| p.x).reduce(f64::max).unwrap();
                let min_y = valid.iter().map(|p| p.y).reduce(f64::min).unwrap();
                let max_y = valid.iter().map(|p| p.y).reduce(f64::max).unwrap();
                l.x = min_x;
                l.y = min_y;
                r.x = max_x;
                r.y = max_y;
                let tl = self.to_screen(sz, &l, vw);
                let br = self.to_screen(sz, &r, vw);
                let mut s = Size::new(br.x - tl.x, br.y - tl.y);
                let mut stroke = stroke;
                let k = 2.0;
                stroke.width = s.width / k;
                s.width = stroke.width;

                frame.stroke(&canvas::Path::rectangle(tl, s), stroke);
            } else {
                if l.is_nan() || r.is_nan() {
                    continue;
                }
                let s = Size::new(stroke.width, stroke.width);
                frame.stroke(
                    &canvas::Path::rectangle(self.to_screen(sz, &l, vw), s),
                    stroke,
                );

                frame.stroke(
                    &canvas::Path::line(self.to_screen(sz, &l, vw), self.to_screen(sz, &r, vw)),
                    stroke,
                );
            }
        }
    }

    pub fn to_screen(&self, sz: iced::Size, v: &Point2D, vw: &Viewport) -> Point {
        let left = vw.left_time.timestamp_f64();
        let width = vw.right_time.timestamp_f64() - left;
        let x = ((v.x - left) / width) * sz.width as f64 * vw.zoom;

        let y = sz.height as f64
            - (v.y - vw.bottom_pos) / (vw.top_pos - vw.bottom_pos) * sz.height as f64;
        if !x.is_finite() {
            panic!("to_screen - x is {:?}", x);
        }
        if !y.is_finite() {
            panic!(
                "to_screen - y is {:?} - top:{:?} - bottom:{}",
                y, vw.top_pos, vw.bottom_pos
            );
        }
        Point::new(x as f32, y as f32)
    }

    #[allow(dead_code)]
    pub fn from_fir(fir: &FirPing) -> Self {
        let mut ret = Self::default();
        let mut x = fir.cfg.start.timestamp_f64();
        let interval = fir.cfg.interval.as_secs_f64();
        for p in fir.rtt.iter() {
            let y = p.0 / p.1;
            ret.points.push(Point2D { x, y });
            x += interval;
        }
        ret
    }
    pub fn from_firdct(fir: &FirPing, zoom: f64, sz: Size, loss: f64) -> Self {
        let width = sz.width as f64 * zoom * 8.0;
        let len = width.ceil() as usize;
        let data = fir.dct.export(len, loss);
        let len = data.len();
        let mut x = fir.cfg.start.timestamp_f64();
        let interval = fir.cfg.interval.as_secs_f64();
        let interval = interval / len as f64 * fir.rtt.len() as f64;
        let mut ret = Self::default();
        for y in data {
            ret.points.push(Point2D { x, y });
            x += interval;
        }
        ret
    }
    #[allow(dead_code)]
    pub fn from_firdctdbg(fir: &FirPing) -> Self {
        let mean = fir.dct.data[0];
        let mut x = fir.cfg.start.timestamp_f64();
        let interval = fir.cfg.interval.as_secs_f64();
        let mut ret = Self::default();
        for y in fir.dct.data.iter().copied() {
            ret.points.push(Point2D {
                x,
                y: (y * 300.0) + mean,
            });
            x += interval;
        }
        ret
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Point2D {
    pub x: f64,
    pub y: f64,
}

impl Point2D {
    pub fn is_nan(&self) -> bool {
        self.x.is_nan() || self.y.is_nan()
    }
    pub fn is_valid(&self) -> bool {
        !self.is_nan()
    }
    pub fn from_point(v: &Ping) -> Self {
        let x = v.received.timestamp_f64();
        let y = v.rtt_us as f64 / 1_000_000.0;
        Self { x, y }
    }

    #[allow(dead_code)]
    pub fn from_fir() {}
}

impl From<&Ping> for Point2D {
    fn from(v: &Ping) -> Self {
        Self::from_point(v)
    }
}
