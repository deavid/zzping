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
}

impl FDQGraph {
    pub fn load_file(&mut self, filename: &str) {
        let timer = Instant::now();
        eprintln!("Loading file: {}", filename);
        self.fd.clear();
        let f = File::open(filename).unwrap();
        let buf = BufReader::new(f);
        let fdreader = FDCodecIter::new(buf);
        for fdq in fdreader {
            self.fd.push(fdq);
        }
        eprintln!("done: {:?}", timer.elapsed());
        self.zoomx = 1.0;
        self.changed = true;
    }
    pub fn update(&mut self, _now: Instant) -> bool {
        let ret = self.changed;
        self.changed = false;
        ret
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
        let f = FrameScaler::new(frame);
        let green = Color::from_rgba8(0, 255, 0, 1.0);
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
        // let green_fill = canvas::Fill::Color(green);

        let space = Path::rectangle(f.pt(0.0, 0.0), f.sz(1.0, 1.0));
        frame.fill(&space, Color::from_rgba8(100, 100, 100, 1.0));
        if self.fd.is_empty() {
            let line = canvas::Path::line(f.pt(0.0, 0.0), f.pt(1.0, 1.0));
            frame.stroke(&line, green_stroke);
        } else {
            let len = (self.fd.len() as f64 / self.zoomx).round() as usize;
            let zero = ((self.fd.len() - len) as f64 * self.posx) as usize;
            let fd = &self.fd[zero..len + zero];
            let frametimes: Vec<_> = fd.iter().map(|x| x.get_timestamp_ms()).collect();
            let min_ftime = frametimes.iter().min().unwrap();
            let max_ftime = frametimes.iter().max().unwrap();
            let max_recv = fd.iter().map(|x| x.recv_us[6]).max().unwrap();
            let src_left = *min_ftime as f64;
            let src_right = *max_ftime as f64;
            let src_top = max_recv as f64;
            let src_bottom = 0.0;

            let points: Vec<_> = self
                .fd
                .iter()
                .map(|x| (x.get_timestamp_ms() as f64, x.recv_us[3] as f64))
                .collect();

            let pa = PlotAssist::new(PlotAssistCfg {
                fs: f,
                src_left,
                src_right,
                src_top,
                src_bottom,
            });

            let fd_first = fd.first().unwrap();
            let fd_last = fd.last().unwrap();

            let line = canvas::Path::line(f.pt(0.0, 1.0 - 1.0 / 2.0), f.pt(1.0, 1.0 - 1.0 / 2.0));
            frame.stroke(&line, black_stroke);
            let line = canvas::Path::line(f.pt(0.0, 1.0 - 1.0 / 4.0), f.pt(1.0, 1.0 - 1.0 / 4.0));
            frame.stroke(&line, black_stroke);
            let line = canvas::Path::line(f.pt(0.0, 1.0 - 1.0 / 16.0), f.pt(1.0, 1.0 - 1.0 / 16.0));
            frame.stroke(&line, black_stroke);

            // let mut src = pa.pt(points[0]);
            let mut path_bldr = path::Builder::new();
            path_bldr.move_to(f.pt(0.0, 1.0));
            for p in points.iter() {
                let dst = pa.pt(*p);
                path_bldr.line_to(dst);
                // let line = canvas::Path::line(src, dst);
                // frame.stroke(&line, green_stroke);
                // src = dst;
            }
            path_bldr.line_to(f.pt(1.0, 1.0));
            path_bldr.close();
            let line = path_bldr.build();
            // frame.fill(&line, green_fill);
            frame.stroke(&line, green_stroke);

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
    }
}
