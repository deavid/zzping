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
use iced::widget::canvas::{self, Cache, Text};
use iced::{Color, Point, Rectangle};
use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct LatencyGraph {
    pub samples: usize,
    pub latency_us: Vec<u32>,
    pub packet_loss_x100_000: Vec<u32>,
    pub current: Instant,
    pub display_address: String,
    pub cache: Cache,
}

impl LatencyGraph {
    pub fn new(display_address: &str, samples: usize) -> Self {
        Self {
            latency_us: vec![],
            packet_loss_x100_000: vec![],
            current: Instant::now(),
            display_address: display_address.to_owned(),
            samples,
            cache: Cache::default(),
        }
    }

    pub fn update(&mut self, now: Instant, stats: &[UdpStats]) -> bool {
        let mut modified = false;
        for s in stats.iter() {
            if s.addr == self.display_address {
                self.latency_us.push(s.avg_time_us.min(500000));
                self.packet_loss_x100_000.push(s.packet_loss_x100_000);
                modified = true;
            }
        }
        while self.latency_us.len() >= self.samples {
            self.latency_us.remove(0);
            self.packet_loss_x100_000.remove(0);
            modified = true;
        }
        if self.current.elapsed() > Duration::from_secs_f32(1.0) {
            modified = true;
        }
        if modified {
            self.current = now;
            self.cache.clear(); // Clear cache when data changes
        }
        modified
    }
}

impl Default for LatencyGraph {
    fn default() -> Self {
        Self::new("", 1000)
    }
}

impl<Message> canvas::Program<Message> for LatencyGraph {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<iced::widget::canvas::Geometry> {
        let geometry = self.cache.draw(renderer, bounds.size(), |frame| {
            // All coordinates inside this closure are local to the frame.
            // (0, 0) is the top-left corner of our canvas.

            let right = frame.width();
            let bottom = frame.height();

            // Create strokes using the new API, matching original alpha values
            let green_stroke = canvas::Stroke::default()
                .with_width(1.0)
                .with_color(Color::from_rgba(0.0, 1.0, 0.0, 0.3));
            let red_stroke = canvas::Stroke::default()
                .with_width(1.0)
                .with_color(Color::from_rgba(1.0, 0.0, 0.0, 0.5));
            let black_stroke1 = canvas::Stroke::default()
                .with_width(3.0)
                .with_color(Color::from_rgba(0.0, 0.0, 0.0, 0.1));
            let black_stroke2 = canvas::Stroke::default()
                .with_width(3.0)
                .with_color(Color::from_rgba(0.0, 0.0, 0.0, 0.2));

            // Fill background. The path must be relative to the frame's origin.
            let background = canvas::Path::rectangle(Point::ORIGIN, frame.size());
            frame.fill(&background, Color::from_rgba(0.7, 0.7, 0.7, 0.2));

            // Draw title text
            let avg_latency: u32 =
                self.latency_us.iter().sum::<u32>() / (self.latency_us.len().max(1) as u32);

            frame.fill_text(Text {
                content: format!(
                    "{} - {:.2}ms avg",
                    self.display_address,
                    avg_latency as f32 / 1000.0
                ),
                position: Point::new(0.0, 0.0), // Position relative to frame
                color: Color::from_rgba(1.0, 1.0, 1.0, 0.9),
                size: iced::Pixels(12.0),
                vertical_alignment: iced::alignment::Vertical::Top,
                horizontal_alignment: iced::alignment::Horizontal::Left,
                ..Text::default()
            });

            if self.latency_us.is_empty() {
                let botright = Point::new(right, bottom);
                let line = canvas::Path::line(Point::ORIGIN, botright);
                frame.stroke(&line, red_stroke);
                // IMPORTANT: Return here to prevent panic from .max().unwrap() on empty vec
                return;
            }

            let ms: f32 = 1000.0;
            // This is now safe because we returned if the vec was empty
            let max = self
                .latency_us
                .iter()
                .filter(|x| **x < 2000000)
                .max()
                .unwrap();

            let len = self.samples;
            let sx = frame.width() / (len as f32);
            let max_sy = frame.height() / (300.0 * ms);
            let sy = ((frame.height() / *max as f32) * 0.8).max(max_sy);

            let y3ms = bottom - 3.0 * ms * sy;
            let y10ms = bottom - 10.0 * ms * sy;
            let y30ms = bottom - 30.0 * ms * sy;
            let y100ms = bottom - 100.0 * ms * sy;

            // Draw reference lines and labels
            if y3ms > 0.0 {
                frame.stroke(
                    &canvas::Path::line(Point::new(0.0, y3ms), Point::new(right, y3ms)),
                    black_stroke1,
                );
                frame.fill_text(Text {
                    content: "3ms".to_string(),
                    position: Point::new(right, y3ms),
                    color: Color::from_rgba(1.0, 1.0, 1.0, 0.9),
                    size: iced::Pixels(12.0),
                    horizontal_alignment: iced::alignment::Horizontal::Right,
                    vertical_alignment: iced::alignment::Vertical::Center,
                    ..Text::default()
                });
            }
            if y10ms > 0.0 {
                frame.stroke(
                    &canvas::Path::line(Point::new(0.0, y10ms), Point::new(right, y10ms)),
                    black_stroke2,
                );
                frame.fill_text(Text {
                    content: "10ms".to_string(),
                    position: Point::new(right, y10ms),
                    color: Color::from_rgba(1.0, 1.0, 1.0, 0.9),
                    size: iced::Pixels(12.0),
                    horizontal_alignment: iced::alignment::Horizontal::Right,
                    vertical_alignment: iced::alignment::Vertical::Center,
                    ..Text::default()
                });
            }
            if y30ms > 0.0 {
                frame.stroke(
                    &canvas::Path::line(Point::new(0.0, y30ms), Point::new(right, y30ms)),
                    black_stroke1,
                );
                frame.fill_text(Text {
                    content: "30ms".to_string(),
                    position: Point::new(right, y30ms),
                    color: Color::from_rgba(1.0, 1.0, 1.0, 0.9),
                    size: iced::Pixels(12.0),
                    horizontal_alignment: iced::alignment::Horizontal::Right,
                    vertical_alignment: iced::alignment::Vertical::Center,
                    ..Text::default()
                });
            }
            if y100ms > 0.0 {
                frame.stroke(
                    &canvas::Path::line(Point::new(0.0, y100ms), Point::new(right, y100ms)),
                    black_stroke2,
                );
                frame.fill_text(Text {
                    content: "100ms".to_string(),
                    position: Point::new(right, y100ms),
                    color: Color::from_rgba(1.0, 1.0, 1.0, 0.9),
                    size: iced::Pixels(12.0),
                    horizontal_alignment: iced::alignment::Horizontal::Right,
                    vertical_alignment: iced::alignment::Vertical::Center,
                    ..Text::default()
                });
            }

            // Draw latency data
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

            // Draw packet loss data
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
        });

        vec![geometry]
    }
}
