// Copyright 2019 Google LLC
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

use super::udp_comm::UdpStats;
use iced::{canvas, Color, Point};
use std::time::{Duration, Instant};

static SAMPLES: usize = 500;

#[derive(Debug)]
pub struct LatencyGraph {
    pub latency_us: Vec<u32>,
    pub packet_loss_x100_000: Vec<u32>,
    pub current: Instant,
    pub display_address: String,
}

impl LatencyGraph {
    pub fn new(display_address: &str) -> Self {
        Self {
            latency_us: vec![],
            packet_loss_x100_000: vec![],
            current: Instant::now(),
            display_address: display_address.to_owned(),
        }
    }
    pub fn update(&mut self, now: Instant, stats: Vec<UdpStats>) -> bool {
        let mut modified = false;
        for s in stats {
            if s.addr == self.display_address {
                self.latency_us.push(s.avg_time_us.min(500000));
                self.packet_loss_x100_000.push(s.packet_loss_x100_000);
                modified = true;
            }
        }
        while self.latency_us.len() >= SAMPLES {
            self.latency_us.remove(0);
            self.packet_loss_x100_000.remove(0);
            modified = true;
        }
        // self.points.push(rand_num());
        if self.current.elapsed() > Duration::from_secs(1) {
            modified = true;
        }
        if modified {
            self.current = now;
        }
        modified
    }
}

impl Default for LatencyGraph {
    fn default() -> Self {
        Self::new("")
    }
}

impl canvas::Drawable for LatencyGraph {
    fn draw(&self, frame: &mut canvas::Frame) {
        use canvas::{Path, Stroke};
        let space = Path::rectangle(Point::new(0.0, 0.0), frame.size());
        let right = frame.width();
        let bottom = frame.height();
        let botright = Point::new(right, bottom);
        let green_stroke = Stroke {
            width: 1.0,
            color: Color::from_rgba8(0, 255, 0, 0.3),
            ..Stroke::default()
        };
        let red_stroke = Stroke {
            width: 1.0,
            color: Color::from_rgba8(255, 0, 0, 0.5),
            ..Stroke::default()
        };
        let black_stroke1 = Stroke {
            width: 3.0,
            color: Color::from_rgba8(0, 0, 0, 0.1),
            ..Stroke::default()
        };
        let black_stroke2 = Stroke {
            width: 3.0,
            color: Color::from_rgba8(0, 0, 0, 0.2),
            ..Stroke::default()
        };

        frame.fill(&space, Color::from_rgba8(100, 100, 100, 1.0));
        if self.latency_us.is_empty() {
            let line = canvas::Path::line(Point::new(0.0, 0.0), botright);
            frame.stroke(&line, red_stroke);
            return;
        }
        let ms: f32 = 1000.0;
        let max = self
            .latency_us
            .iter()
            .filter(|x| **x < 2000000)
            .max()
            .unwrap();
        // let len = self.points.len();
        let len = SAMPLES;
        let sx = frame.width() / (len as f32);
        let max_sy = frame.height() / (300.0 * ms);
        let sy = ((frame.height() / *max as f32) * 0.8).max(max_sy);

        let y3ms = bottom - 3.0 * ms * sy;
        let y10ms = bottom - 10.0 * ms * sy;
        let y30ms = bottom - 30.0 * ms * sy;
        let y100ms = bottom - 100.0 * ms * sy;
        if y3ms > 0.0 {
            frame.stroke(
                &canvas::Path::line(Point::new(0.0, y3ms), Point::new(right, y3ms)),
                black_stroke1,
            )
        }
        if y10ms > 0.0 {
            frame.stroke(
                &canvas::Path::line(Point::new(0.0, y10ms), Point::new(right, y10ms)),
                black_stroke2,
            )
        }
        if y30ms > 0.0 {
            frame.stroke(
                &canvas::Path::line(Point::new(0.0, y30ms), Point::new(right, y30ms)),
                black_stroke1,
            )
        }
        if y100ms > 0.0 {
            frame.stroke(
                &canvas::Path::line(Point::new(0.0, y100ms), Point::new(right, y100ms)),
                black_stroke2,
            )
        }
        let mut oldp: Option<Point> = None;
        for (n, p) in self.latency_us.iter().enumerate() {
            let x = n as f32 * sx;
            let y = bottom - *p as f32 * sy;
            let point = Point::new(x, y);
            if let Some(oldp) = oldp {
                let line = canvas::Path::line(oldp, point);
                frame.stroke(&line, green_stroke);
            }
            if n == len - 1 {
                let x2 = frame.width();
                let p2 = Point::new(x2, y);
                let line = canvas::Path::line(point, p2);
                frame.stroke(&line, green_stroke);
            }
            oldp = Some(point);
        }
        // Packet Loss:
        let sy: f32 = (frame.height() / 100000.0) * 1.0;
        let mut oldp: Option<Point> = None;
        for (n, p) in self.packet_loss_x100_000.iter().enumerate() {
            let x = n as f32 * sx;
            let y = bottom - *p as f32 * sy;
            let point = Point::new(x, y);
            if let Some(oldp) = oldp {
                let line = canvas::Path::line(oldp, point);
                frame.stroke(&line, red_stroke);
            }
            if n == len - 1 {
                let x2 = frame.width();
                let p2 = Point::new(x2, y);
                let line = canvas::Path::line(point, p2);
                frame.stroke(&line, red_stroke);
            }
            oldp = Some(point);
        }
    }
}
