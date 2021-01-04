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
    pub fn ptx(&self, px: f64, py: f64) -> Point {
        let ptp = self.ptp((px, py));
        self.cfg.fs.pt(ptp.0 as f32, py as f32)
    }
}

#[derive(Debug, Default)]
pub struct FDQGraph {
    fd: Vec<FrameDataQ<Complete>>,
    fdcache: Vec<(i64, Vec<FrameDataQ<Complete>>)>,
    changed: bool,
    zoomx: f64,
    posx: f64,
    zoomy: f64,
    max_recv: i64,
    max_inflight: f32,
    max_lostpackets: f32,
    scale_factor: f64,
}

impl FDQGraph {
    pub fn load_file(&mut self, filename: &str) {
        let timer = Instant::now();
        eprintln!("Loading file: {}", filename);
        // self.fd.clear();
        let f = File::open(filename).unwrap();
        let buf = BufReader::new(f);
        let fdreader = FDCodecIter::new(buf);
        let mut fd: Vec<FrameDataQ<Complete>> = Vec::with_capacity(10000);
        self.max_recv = 0;
        self.max_inflight = 0.0;
        self.max_lostpackets = 0.0;
        let mut stdmean_inflight: f32 = 0.0;
        let mut stdmean_lostpackets: f32 = 0.0;
        let mut timer_rm = Instant::now();
        for mut fdq in fdreader {
            self.max_inflight = self.max_inflight.max(fdq.inflight);
            stdmean_inflight += (fdq.inflight as f32).powi(2);
            self.max_lostpackets = self.max_lostpackets.max(fdq.lost_packets);
            stdmean_lostpackets += (fdq.lost_packets as f32).powi(2);
            self.max_recv = self.max_recv.max(fdq.recv_us[6]);
            if fdq.recv_us_len == 0 {
                fdq.recv_us = [0, 0, 0, 0, 0, 0, 0];
            }

            fd.push(fdq);
            if timer_rm.elapsed().as_secs() >= 1 {
                timer_rm = Instant::now();
                eprintln!("Still loading... got {} items now.", fd.len());
            }
        }
        dbg!(fd.len());
        stdmean_inflight /= fd.len() as f32;
        stdmean_inflight = stdmean_inflight.sqrt();
        dbg!(stdmean_inflight);
        dbg!(self.max_inflight);
        dbg!(stdmean_lostpackets);
        dbg!(self.max_lostpackets);
        self.fd = fd.clone(); // fd.chunks(1000).map(|x| FrameDataQ::fold_vec(x)).collect();
        eprintln!("loaded, caching: {:?}", timer.elapsed());
        let timer = Instant::now();
        self.fdcache.clear();

        let mut step = 1;
        for _ in 0..16 {
            step *= 2;
            fd = fd.chunks(2).map(|x| FrameDataQ::fold_vec(x)).collect();
            self.fdcache.push((step, fd.clone()));
            if timer_rm.elapsed().as_secs() >= 1 {
                timer_rm = Instant::now();
                eprintln!(
                    "Still caching... step {} with {} items now.",
                    step,
                    fd.len()
                );
            }

            if fd.len() < 1000 {
                break;
            }
        }
        self.zoomx = 1.0;
        self.zoomy = 1.0;
        self.posx = 0.5;
        self.scale_factor = 1.0;
        self.changed = true;
        eprintln!("caching finished: {:?}", timer.elapsed());
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
        if (x - self.posx).abs() > 1e-12 {
            self.posx = x;
            self.changed = true;
        }
    }
    pub fn set_scalefactor(&mut self, z: f64) {
        self.scale_factor = z;
        self.changed = true;
    }
}

impl canvas::Drawable for FDQGraph {
    fn draw(&self, frame: &mut canvas::Frame) {
        let timer_begin = Instant::now();
        let f = FrameScaler::new(frame);
        let color_r0 = Color::from_rgba8(100, 50, 50, 1.0);
        let color_r1 = Color::from_rgba8(220, 50, 50, 1.0);
        let color_r2 = Color::from_rgba8(200, 150, 50, 1.0);
        let color_r3 = Color::from_rgba8(200, 200, 50, 1.0);
        let color_r4 = Color::from_rgba8(50, 220, 50, 1.0);
        let color_r5 = Color::from_rgba8(50, 200, 200, 1.0);
        let color_r6 = Color::from_rgba8(50, 150, 200, 1.0);
        // let color_r6 = Color::from_rgba8(70, 100, 200, 1.0);
        let color_inflight = Color::from_rgba8(0, 0, 0, 0.3);

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
        let fill_r0 = canvas::Fill::Color(color_r0);
        let fill_r1 = canvas::Fill::Color(color_r1);
        let fill_r2 = canvas::Fill::Color(color_r2);
        let fill_r3 = canvas::Fill::Color(color_r3);
        let fill_r4 = canvas::Fill::Color(color_r4);
        let fill_r5 = canvas::Fill::Color(color_r5);
        let fill_r6 = canvas::Fill::Color(color_r6);
        let fill_recv = vec![
            fill_r0, fill_r1, fill_r2, fill_r3, fill_r4, fill_r5, fill_r6,
        ];
        let fill_inflight = canvas::Fill::Color(color_inflight);

        let space = Path::rectangle(f.pt(0.0, 0.0), f.sz(1.0, 1.0));
        frame.fill(&space, Color::from_rgba8(100, 100, 100, 1.0));
        if self.fd.is_empty() {
            let line = canvas::Path::line(f.pt(0.0, 0.0), f.pt(1.0, 1.0));
            frame.stroke(&line, green_stroke);
        } else {
            let total_sublimit = 3;
            let total_limit = 500 * total_sublimit;
            let total_cache = total_limit * 3 / 2;
            let mut fd = &self.fd;
            let mut cache_step = 1;
            for (s, cache) in self.fdcache.iter() {
                if cache.len() / self.zoomx as usize > total_cache {
                    fd = cache;
                    cache_step = *s;
                }
            }
            let zoomx = self.zoomx.min(fd.len() as f64 / 2.0);
            let len = (fd.len() as f64 / zoomx).round() as usize;
            let zero = ((fd.len() - len) as f64 * self.posx) as usize;
            let ifd = &fd[zero..len + zero];

            let step = (ifd.len() / total_limit).max(1);
            let substep = (ifd.len() * total_sublimit / total_limit)
                .min(total_sublimit)
                .max(1);
            // TODO: Grey out areas w/o packets. These appear as lines now and seem to have "data", but they don't.
            let time_chunks = Instant::now();
            let fd: Vec<_> = match step > 1 {
                // FIXME: if a chunk is all -1, then what happens? think of zooming in a conn-loss.
                true => ifd.chunks(step).map(|x| FrameDataQ::fold_vec(x)).collect(),
                false => ifd.iter().copied().collect(), // .filter(|x| x.recv_us_len > 0)
            };
            if time_chunks.elapsed().as_millis() > 10 {
                dbg!(time_chunks.elapsed());
                eprintln!("cache: {} step: {} substep: {}", cache_step, step, substep);
            }
            let time_windows = Instant::now();
            let fd: Vec<_> = match substep > 1 {
                // FIXME: if a window is all -1, then what happens? think of zooming in a conn-loss.
                true => fd
                    .windows(substep)
                    .map(|x| FrameDataQ::fold_vec(x))
                    .collect(),
                false => fd.iter().copied().collect(),
            };
            if time_windows.elapsed().as_millis() > 10 {
                dbg!(time_windows.elapsed());
            }

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

            let scale_factor = self.scale_factor;
            let src_left = min_ftime as f64;
            let src_right = max_ftime as f64;
            let src_top = (self.max_recv as f64 * 1.5 / self.zoomy).powf(scale_factor);
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
                    .map(|x| {
                        (
                            x.get_timestamp_ms() as f64,
                            (x.recv_us[i] as f64).max(0.0).powf(scale_factor),
                        )
                    })
                    .collect();
                points.push(points_i)
            }

            let line = canvas::Path::line(f.pt(0.0, 1.0 - 1.0 / 2.0), f.pt(1.0, 1.0 - 1.0 / 2.0));
            frame.stroke(&line, black_stroke);
            let line = canvas::Path::line(f.pt(0.0, 1.0 - 1.0 / 4.0), f.pt(1.0, 1.0 - 1.0 / 4.0));
            frame.stroke(&line, black_stroke);
            let line = canvas::Path::line(f.pt(0.0, 1.0 - 1.0 / 16.0), f.pt(1.0, 1.0 - 1.0 / 16.0));
            frame.stroke(&line, black_stroke);

            let mut path_bldr: Vec<_> = (0..7).map(|_| path::Builder::new()).collect();
            path_bldr.iter_mut().for_each(|b| b.move_to(f.pt(0.0, 1.0)));
            let mut path_inflight = path::Builder::new();
            path_inflight.move_to(f.pt(0.0, 1.0));

            let section_limit = 50;
            let mut line_count = 0;
            for (n, fp) in fd.iter().enumerate() {
                path_inflight.line_to(pa.ptx(
                    fp.get_timestamp_ms() as f64,
                    1.0 - (fp.inflight as f64 * 10.0 / self.max_inflight as f64).tanh(),
                ));
                path_bldr
                    .iter_mut()
                    .zip(points.iter())
                    .for_each(|(b, p)| b.line_to(pa.pt(p[n])));

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

                    line_count = 1;
                }
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
            path_inflight.line_to(f.pt(1.0, 1.0));
            path_inflight.close();
            let poly = path_inflight.build();
            frame.fill(&poly, fill_inflight);

            let fd_first = fd.first().unwrap();
            let fd_last = fd.last().unwrap();
            let mid_pos = ((fd.len() - 1) as f32 * self.posx as f32).round();
            let fd_mid = fd[mid_pos as usize];

            // Zoom X locator
            let line = canvas::Path::line(f.pt(self.posx as f32, 0.0), f.pt(self.posx as f32, 1.0));
            frame.stroke(&line, black_stroke);

            // Ping timing lines - vertical
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
                content: format!(
                    "Viewport width: {}\n{}",
                    vw_width_text,
                    fd_mid.get_datetime()
                ),
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
                content: format!(
                    "{} - {:.2}ms",
                    fd_last.get_datetime(),
                    src_top.powf(scale_factor.recip()) / 1000.0
                ),
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
                content: format!(
                    "{:.2}ms",
                    (src_top / 2.0).powf(scale_factor.recip()) / 1000.0
                ),
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
                content: format!(
                    "{:.2}ms",
                    (src_top / 4.0).powf(scale_factor.recip()) / 1000.0
                ),
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
                content: format!(
                    "{:.2}ms",
                    (src_top / 16.0).powf(scale_factor.recip()) / 1000.0
                ),
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
        if timer_begin.elapsed().as_millis() > 50 {
            dbg!(timer_begin.elapsed());
        }
    }
}
