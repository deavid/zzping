// Copyright 2021 Google LLC
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
    widget::canvas::{self, path, Path, Stroke, Cache, Text},
    Color, Point, Size, Vector, Rectangle,
};
use zzping_lib::framedataq::{Complete, FDCodecIter, FrameDataQ, IterFold, SubSecType};

use crate::gui::Message;

#[derive(Debug, Default, Copy, Clone)]
pub struct FrameScaler {
    fwidth: f32,
    fheight: f32,
}

impl FrameScaler {
    pub fn new(rect: &iced::Rectangle) -> Self {
        Self {
            fwidth: rect.width,
            fheight: rect.height,
            // ..Default::default()
        }
    }
    pub fn pt(&self, x: f32, y: f32) -> Point {
        Point::new(x * self.fwidth, y * self.fheight)
    }
    pub fn sz(&self, x: f32, y: f32) -> Size {
        Size::new(x * self.fwidth, y * self.fheight)
    }
    pub fn pwh(&self, h: f32) -> f32 {
        (h * (self.fheight.powi(2) + (2.0 * self.fwidth).powi(2)).sqrt()).sqrt()
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
    tp: f64,
    // rght: f64,
    // btm: f64,
}

impl PlotAssist {
    pub fn new(cfg: PlotAssistCfg) -> Self {
        let wdth = cfg.src_right - cfg.src_left;
        let hght = cfg.src_bottom - cfg.src_top;
        let lft = cfg.src_left / wdth;
        let tp = cfg.src_top / hght;
        // let rght = cfg.src_right / wdth;
        // let btm = cfg.src_bottom / hght;

        Self {
            cfg,
            wdth,
            hght,
            lft,
            tp,
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

#[derive(Debug)]
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
    cache: Cache,
}

impl Default for FDQGraph {
    fn default() -> Self {
        Self {
            fd: Default::default(),
            fdcache: Default::default(),
            changed: Default::default(),
            zoomx: Default::default(),
            posx: Default::default(),
            zoomy: Default::default(),
            max_recv: Default::default(),
            max_inflight: Default::default(),
            max_lostpackets: Default::default(),
            scale_factor: Default::default(),
            cache: Cache::new(),
        }
    }
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
        // These should have a min of 1.0 to prevent div/0 err
        self.max_inflight = 1.0;
        self.max_lostpackets = 1.0;
        let mut stdmean_inflight: f32 = 0.0;
        let mut stdmean_lostpackets: f32 = 0.0;
        let mut timer_rm = Instant::now();
        let mut last_ts = 0;
        let mut last_dt = None;
        for mut fdq in fdreader {
            self.max_inflight = self.max_inflight.max(fdq.inflight);
            stdmean_inflight += fdq.inflight.powi(2);
            self.max_lostpackets = self.max_lostpackets.max(fdq.lost_packets);
            stdmean_lostpackets += fdq.lost_packets.powi(2);
            self.max_recv = self.max_recv.max(fdq.recv_us[6]);
            if fdq.recv_us_len == 0 {
                fdq.recv_us = [0, 0, 0, 0, 0, 0, 0];
            }
            let gap = fdq.timestamp.unwrap() - last_ts;
            if last_ts > 0 && gap > 10 {
                eprintln!(
                    "Found a gap of {:.2}h between {} and {}",
                    gap as f32 / 60.0 / 60.0,
                    last_dt.unwrap(),
                    fdq.get_datetime()
                );
                for ts in (last_ts + 1)..(fdq.timestamp.unwrap() - 1) {
                    let new_fdq = FrameDataQ::<Complete> {
                        phantom: Default::default(),
                        timestamp: Some(ts),
                        subsec_ms: SubSecType::Abs(0),
                        inflight: 0.0,
                        lost_packets: 1.0,
                        recv_us_len: 0,
                        recv_us: [0, 0, 0, 0, 0, 0, 0],
                    };
                    fd.push(new_fdq);
                }
            }
            fd.push(fdq);
            if timer_rm.elapsed().as_secs() >= 1 {
                timer_rm = Instant::now();
                eprintln!("Still loading... got {} items now.", fd.len());
            }
            last_ts = fdq.timestamp.unwrap();
            last_dt = Some(fdq.get_datetime());
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
        for _ in 0..4 {
            step *= 32;
            fd = fd.iter().cloned().iter_fold(32, 8).collect();
            self.fdcache.push((step, fd.clone()));
            if timer_rm.elapsed().as_secs() >= 1 {
                timer_rm = Instant::now();
                eprintln!(
                    "Still caching... step {} with {} items now.",
                    step,
                    fd.len()
                );
            }

            if fd.len() < 8000 {
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

fn fill_color(color: Color) -> iced::widget::canvas::Fill {
    iced::widget::canvas::Fill {
        style: color.into(),
        rule: iced::widget::canvas::fill::Rule::NonZero,
    }
}

impl<Message> canvas::Program<Message> for FDQGraph {
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
            // Simple placeholder implementation for now
            let background = canvas::Path::rectangle(Point::new(0.0, 0.0), bounds.size());
            let fill = iced::widget::canvas::Fill {
                style: Color::from_rgba8(60, 60, 60, 1.0).into(),
                rule: iced::widget::canvas::fill::Rule::NonZero,
            };
            frame.fill(&background, fill);
            
            // Add placeholder text
            frame.fill_text(iced::widget::canvas::Text {
                content: "FDQ Graph - Migration in Progress".to_string(),
                position: Point::new(bounds.width / 2.0, bounds.height / 2.0),
                color: Color::WHITE,
                size: iced::Pixels(16.0),
                font: iced::Font::default(),
                horizontal_alignment: iced::alignment::Horizontal::Center,
                vertical_alignment: iced::alignment::Vertical::Center,
                line_height: iced::widget::text::LineHeight::default(),
                shaping: iced::widget::text::Shaping::default(),
            });
        });
        
        vec![geometry]
    }
}
