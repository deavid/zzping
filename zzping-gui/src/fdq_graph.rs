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

use std::{fs::File, io::BufReader, time::Instant};

use iced::{
    canvas::{self, path, Path, Stroke},
    Color, Point, Size, Vector,
};
use zzpinglib::framedataq::{Complete, FDCodecIter, FrameDataQ};

#[derive(Debug, Default, Copy, Clone)]
pub struct FrameScaler {
    fwidth: f32,
    fheight: f32,
}

impl FrameScaler {
    pub fn new(frame: &canvas::Frame) -> Self {
        Self {
            fwidth: frame.width(),
            fheight: frame.height(),
            // ..Default::default()
        }
    }
    pub fn pt(&self, x: f32, y: f32) -> Point {
        Point::new(x * self.fwidth, y * self.fheight)
    }
    pub fn sz(&self, x: f32, y: f32) -> Size {
        Size::new(x * self.fwidth, y * self.fheight)
    }
    pub fn ph(&self, h: f32) -> f32 {
        h * self.fheight
    }
    pub fn _pw(&self, w: f32) -> f32 {
        w * self.fwidth
    }
}

#[derive(Debug, Default, Copy, Clone)]
pub struct PlotAssistCfg {
    fs: FrameScaler,
    src_left: f64,
    src_right: f64,
    src_top: f64,
    src_bottom: f64,
}

#[derive(Debug, Default, Copy, Clone)]
pub struct PlotAssist {
    cfg: PlotAssistCfg,
    wdth: f64,
    hght: f64,
    lft: f64,
    rght: f64,
    tp: f64,
    btm: f64,
}

impl PlotAssist {
    pub fn new(cfg: PlotAssistCfg) -> Self {
        let wdth = cfg.src_right - cfg.src_left;
        let hght = cfg.src_bottom - cfg.src_top;
        let lft = cfg.src_left / wdth;
        let rght = cfg.src_right / wdth;
        let tp = cfg.src_top / hght;
        let btm = cfg.src_bottom / hght;

        Self {
            cfg,
            wdth,
            hght,
            lft,
            rght,
            tp,
            btm,
        }
    }
    pub fn ptp(&self, p: (f64, f64)) -> (f64, f64) {
        // let x = (x - self.src_left) / w;
        let x = p.0 / self.wdth - self.lft;
        // let y = (y - self.src_top) / h;
        let y = p.1 / self.hght - self.tp;
        (x, y)
    }
    pub fn pt(&self, p: (f64, f64)) -> Point {
        let ptp = self.ptp(p);
        self.cfg.fs.pt(ptp.0 as f32, ptp.1 as f32)
    }
}

#[derive(Debug, Default)]
pub struct FDQGraph {
    pub fd: Vec<FrameDataQ<Complete>>,
    changed: bool,
    zoomx: f64,
    posx: f64,
    zoomy: f64,
    max_recv: i64,
}

impl FDQGraph {
    pub fn load_file(&mut self, filename: &str) {
        let timer = Instant::now();
        eprintln!("Loading file: {}", filename);
        self.fd.clear();
        let f = File::open(filename).unwrap();
        let buf = BufReader::new(f);
        let fdreader = FDCodecIter::new(buf);
        self.max_recv = 0;
        for fdq in fdreader {
            self.max_recv = self.max_recv.max(fdq.recv_us[6]);
            self.fd.push(fdq);
        }
        eprintln!("done: {:?}", timer.elapsed());

        self.zoomx = 1.0;
        self.zoomy = 1.0;
        self.changed = true;
    }
    pub fn update(&mut self, _now: Instant) -> bool {
        let ret = self.changed;
        self.changed = false;
        ret
    }
    pub fn set_zoomy(&mut self, z: f64) {
        self.zoomy = z;
        self.changed = true;
    }
    pub fn set_zoomx(&mut self, z: f64) {
        self.zoomx = z;
        self.changed = true;
    }
    pub fn set_posx(&mut self, x: f64) {
        self.posx = x;
        self.changed = true;
    }
}

impl canvas::Drawable for FDQGraph {
    fn draw(&self, frame: &mut canvas::Frame) {
        let timer_begin = Instant::now();
        let f = FrameScaler::new(frame);
        let color_r6 = Color::from_rgba8(200, 100, 50, 1.0);
        let color_r5 = Color::from_rgba8(200, 150, 50, 1.0);
        let color_r4 = Color::from_rgba8(200, 200, 50, 1.0);
        let color_r3 = Color::from_rgba8(50, 220, 50, 1.0);
        let color_r2 = Color::from_rgba8(50, 100, 200, 1.0);
        let color_r1 = Color::from_rgba8(50, 50, 50, 1.0);
        let green10 = Color::from_rgba8(0, 255, 0, 0.1);
        let white90 = Color::from_rgba8(255, 255, 255, 0.9);
        let black90 = Color::from_rgba8(0, 0, 0, 0.9);
        let black50 = Color::from_rgba8(0, 0, 0, 0.5);
        let green_stroke = Stroke {
            width: 1.0,
            color: green10,
            ..Stroke::default()
        };
        let black_stroke = Stroke {
            width: 0.5,
            color: black50,
            ..Stroke::default()
        };
        let fill_r1 = canvas::Fill::Color(color_r1);
        let fill_r2 = canvas::Fill::Color(color_r2);
        let fill_r3 = canvas::Fill::Color(color_r3);
        let fill_r4 = canvas::Fill::Color(color_r4);
        let fill_r5 = canvas::Fill::Color(color_r5);
        let fill_r6 = canvas::Fill::Color(color_r6);
        let fill_recv = vec![fill_r1, fill_r2, fill_r3, fill_r4, fill_r5, fill_r6];
        let space = Path::rectangle(f.pt(0.0, 0.0), f.sz(1.0, 1.0));
        frame.fill(&space, Color::from_rgba8(100, 100, 100, 1.0));
        if self.fd.is_empty() {
            let line = canvas::Path::line(f.pt(0.0, 0.0), f.pt(1.0, 1.0));
            frame.stroke(&line, green_stroke);
        } else {
            let total_limit = 2000;

            let zoomx = self.zoomx.min(self.fd.len() as f64 / 2.0);
            let len = (self.fd.len() as f64 / zoomx).round() as usize;
            let zero = ((self.fd.len() - len) as f64 * self.posx) as usize;
            let ifd = &self.fd[zero..len + zero];

            let step = if ifd.len() > total_limit {
                ifd.len() / total_limit
            } else {
                1
            };

            let fd: Vec<_> = ifd.iter().step_by(step).collect();
            let min_ftime = fd
                .iter()
                .take(100)
                .map(|x| x.get_timestamp_ms())
                .min()
                .unwrap();
            let max_ftime = fd[(fd.len() as i64 - 100).max(0) as usize..]
                .iter()
                .map(|x| x.get_timestamp_ms())
                .max()
                .unwrap();
            let max_recv = self.max_recv;
            let src_left = min_ftime as f64;
            let src_right = max_ftime as f64;
            let src_top = max_recv as f64 / self.zoomy;
            let src_bottom = 0.0;

            let pa = PlotAssist::new(PlotAssistCfg {
                fs: f,
                src_left,
                src_right,
                src_top,
                src_bottom,
            });
            let mut points: Vec<_> = vec![];
            for i in 0..7 {
                let points_i: Vec<_> = fd
                    .iter()
                    .map(|x| (x.get_timestamp_ms() as f64, x.recv_us[i] as f64))
                    .collect();
                points.push(points_i)
            }
            // let points_3 = &points[3];
            // let points_6 = &points[6];

            // dbg!(pa.ptp(points[0]), pa.ptp(*points.last().unwrap()));

            let fd_first = fd.first().unwrap();
            let fd_last = fd.last().unwrap();

            let line = canvas::Path::line(f.pt(0.0, 1.0 - 1.0 / 2.0), f.pt(1.0, 1.0 - 1.0 / 2.0));
            frame.stroke(&line, black_stroke);
            let line = canvas::Path::line(f.pt(0.0, 1.0 - 1.0 / 4.0), f.pt(1.0, 1.0 - 1.0 / 4.0));
            frame.stroke(&line, black_stroke);
            let line = canvas::Path::line(f.pt(0.0, 1.0 - 1.0 / 16.0), f.pt(1.0, 1.0 - 1.0 / 16.0));
            frame.stroke(&line, black_stroke);

            // let mut src = pa.pt(points[0]);
            let mut path_bldr: Vec<_> = (0..7).map(|_| path::Builder::new()).collect();
            path_bldr.iter_mut().for_each(|b| b.move_to(f.pt(0.0, 1.0)));

            // let mut path_bldr_3 = path::Builder::new();
            // path_bldr_3.move_to(f.pt(0.0, 1.0));
            // let mut path_bldr_6 = path::Builder::new();
            // path_bldr_6.move_to(f.pt(0.0, 1.0));

            let section_limit = 50;
            let mut line_count = 0;
            for (n, _) in fd.iter().enumerate() {
                path_bldr
                    .iter_mut()
                    .zip(points.iter())
                    .for_each(|(b, p)| b.line_to(pa.pt(p[n])));
                // let p3 = points_3[n];
                // let p6 = points_6[n];
                // let dst3 = pa.pt(p3);
                // let dst6 = pa.pt(p6);
                // path_bldr_3.line_to(dst3);
                // path_bldr_6.line_to(dst6);
                line_count += 1;
                if line_count > section_limit {
                    path_bldr
                        .iter_mut()
                        .zip(points.iter())
                        .zip(fill_recv.iter())
                        .rev()
                        .for_each(|((b, p), fill)| {
                            let p = p[n];
                            let mid = f.pt(pa.ptp(p).0 as f32, 1.0);
                            b.line_to(mid);
                            b.close();
                            let old_b = std::mem::replace(b, path::Builder::new());
                            let polygon = old_b.build();
                            frame.fill(&polygon, *fill);
                            b.move_to(mid);
                            b.line_to(pa.pt(p));
                        });

                    // let mid6 = f.pt(pa.ptp(p6).0 as f32, 1.0);
                    // path_bldr_6.line_to(mid6);
                    // path_bldr_6.close();
                    // let line = path_bldr_6.build();
                    // frame.fill(&line, fill_r6);
                    // path_bldr_6 = path::Builder::new();
                    // path_bldr_6.move_to(mid6);
                    // path_bldr_6.line_to(dst6);

                    // let mid3 = f.pt(pa.ptp(p3).0 as f32, 1.0);
                    // path_bldr_3.line_to(mid3);
                    // path_bldr_3.close();
                    // let line = path_bldr_3.build();
                    // frame.fill(&line, fill_r3);
                    // path_bldr_3 = path::Builder::new();
                    // path_bldr_3.move_to(mid3);
                    // path_bldr_3.line_to(dst3);

                    line_count = 1;
                }
                // let line = canvas::Path::line(src, dst);
                // frame.stroke(&line, green_stroke);
                // src = dst;
            }
            path_bldr
                .iter_mut()
                .zip(fill_recv.iter())
                .rev()
                .for_each(|(b, fill)| {
                    b.line_to(f.pt(1.0, 1.0));
                    b.close();
                    // We need to replace it with a new, even if its not used.
                    let old_b = std::mem::replace(b, path::Builder::new());
                    let polygon = old_b.build();
                    frame.fill(&polygon, *fill);
                });

            // path_bldr_6.line_to(f.pt(1.0, 1.0));
            // path_bldr_6.close();
            // let line = path_bldr_6.build();
            // frame.fill(&line, fill_r6);

            // path_bldr_3.line_to(f.pt(1.0, 1.0));
            // path_bldr_3.close();
            // let line = path_bldr_3.build();
            // frame.fill(&line, fill_r3);

            // frame.stroke(&line, green_stroke);

            let line = canvas::Path::line(f.pt(0.0, 1.0 - 1.0 / 2.0), f.pt(1.0, 1.0 - 1.0 / 2.0));
            frame.stroke(&line, black_stroke);
            let line = canvas::Path::line(f.pt(0.0, 1.0 - 1.0 / 4.0), f.pt(1.0, 1.0 - 1.0 / 4.0));
            frame.stroke(&line, black_stroke);
            let line = canvas::Path::line(f.pt(0.0, 1.0 - 1.0 / 16.0), f.pt(1.0, 1.0 - 1.0 / 16.0));
            frame.stroke(&line, black_stroke);
            let vw_width = (fd_last.get_datetime() - fd_first.get_datetime())
                .to_std()
                .unwrap();
            let vw_width_text = match vw_width.as_secs() {
                3601..=u64::MAX => format!("{:.2}h", vw_width.as_secs_f32() / 60.0 / 60.0),
                120..=3600 => format!("{:.2}min", vw_width.as_secs_f32() / 60.0),
                _ => format!("{:?}", vw_width),
            };
            let shadow = Vector::new(2.0, 1.0);
            let text = canvas::Text {
                content: format!("{}", fd_first.get_datetime()),
                position: f.pt(0.01, 0.01),
                color: white90,
                size: f.ph(0.04),
                font: iced::Font::Default,
                horizontal_alignment: iced::HorizontalAlignment::Left,
                vertical_alignment: iced::VerticalAlignment::Top,
            };
            frame.fill_text(canvas::Text {
                color: black90,
                position: text.position + shadow,
                ..text.clone()
            });
            frame.fill_text(text);
            let text = canvas::Text {
                content: format!("Viewport width: {}", vw_width_text),
                position: f.pt(0.5, 0.01),
                color: white90,
                size: f.ph(0.04),
                font: iced::Font::Default,
                horizontal_alignment: iced::HorizontalAlignment::Center,
                vertical_alignment: iced::VerticalAlignment::Top,
            };
            frame.fill_text(canvas::Text {
                color: black90,
                position: text.position + shadow,
                ..text.clone()
            });
            frame.fill_text(text);
            let text = canvas::Text {
                content: format!("{} - {:.2}ms", fd_last.get_datetime(), src_top / 1000.0),
                position: f.pt(0.99, 0.01),
                color: white90,
                size: f.ph(0.04),
                font: iced::Font::Default,
                horizontal_alignment: iced::HorizontalAlignment::Right,
                vertical_alignment: iced::VerticalAlignment::Top,
            };
            frame.fill_text(canvas::Text {
                color: black90,
                position: text.position + shadow,
                ..text.clone()
            });
            frame.fill_text(text);
            let text = canvas::Text {
                content: format!("{:.2}ms", src_top / 2000.0),
                position: f.pt(0.99, 0.5),
                color: white90,
                size: f.ph(0.04),
                font: iced::Font::Default,
                horizontal_alignment: iced::HorizontalAlignment::Right,
                vertical_alignment: iced::VerticalAlignment::Center,
            };
            frame.fill_text(canvas::Text {
                color: black90,
                position: text.position + shadow,
                ..text.clone()
            });
            frame.fill_text(text);
            let text = canvas::Text {
                content: format!("{:.2}ms", src_top / 4000.0),
                position: f.pt(0.99, 0.75),
                color: white90,
                size: f.ph(0.04),
                font: iced::Font::Default,
                horizontal_alignment: iced::HorizontalAlignment::Right,
                vertical_alignment: iced::VerticalAlignment::Center,
            };
            frame.fill_text(canvas::Text {
                color: black90,
                position: text.position + shadow,
                ..text.clone()
            });
            frame.fill_text(text);
            let text = canvas::Text {
                content: format!("{:.2}ms", src_top / 16000.0),
                position: f.pt(0.99, 1.0 - 1.0 / 16.0),
                color: white90,
                size: f.ph(0.04),
                font: iced::Font::Default,
                horizontal_alignment: iced::HorizontalAlignment::Right,
                vertical_alignment: iced::VerticalAlignment::Center,
            };
            frame.fill_text(canvas::Text {
                color: black90,
                position: text.position + shadow,
                ..text.clone()
            });
            frame.fill_text(text);
        }
        dbg!(timer_begin.elapsed());
    }
}
