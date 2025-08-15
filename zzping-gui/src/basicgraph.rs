use std::time::Duration;

use iced::{
    canvas::{event, Frame, Program, Stroke},
    Color,
};
use zzping_lib::pingdata::{FirPing, FirPingConfig, Ping};

use crate::{
    firtest::Msg,
    graphutils::{PointGraph, Viewport},
};

#[derive(Debug, Default)]
pub struct Graph {
    pub firdata: FirPing,
    pub firdata2: FirPing,
    pub firdata3: FirPing,
    pub raw_pings: PointGraph,
    pub mean_pings: PointGraph,
    pub stddevup_pings: PointGraph,
    pub stddevdown_pings: PointGraph,
    pub data: Vec<Ping>,
    pub vw: Viewport,
    pub size: Option<iced::Size>,
    pub test: f64,
}

impl Graph {
    pub fn load(&mut self, data: Vec<Ping>) {
        self.data = data;
        self.vw = Viewport::default();

        //for p in self.data.iter().take(7300).skip(7100) {
        for p in self.data.iter().take(1_000_000) {
            let t = p.received;
            let r = p.rtt_us;
            self.vw.extend(t, r as f64 / 1_000_000.0);
            self.raw_pings.points.push(p.into());
        }
        let msec = 10;
        let cfg = FirPingConfig {
            start: self.vw.left_time,
            end: self.vw.right_time,
            interval: Duration::from_millis(msec),
            window_size: Duration::from_millis(msec * 11),
            sigmas: 6.0,
        };
        self.firdata3 = FirPing::from_pings(&cfg, Duration::from_millis(10), &self.data, 0);
        let cfg = FirPingConfig {
            start: self.vw.left_time,
            end: self.vw.right_time,
            interval: Duration::from_millis(msec * 32),
            window_size: Duration::from_millis(msec * 128),
            sigmas: 6.0,
        };
        let w = 7;
        self.firdata2 = FirPing::from_pings(&cfg, Duration::from_millis(10), &self.data, w);
        self.firdata = FirPing::from_pings(&cfg, Duration::from_millis(10), &self.data, -w);

        self.redraw();

        dbg!(self.firdata3.rtt.len());
    }
    pub fn redraw(&mut self) {
        if let Some(sz) = self.size {
            if !self.firdata3.dct.data.is_empty() {
                self.mean_pings =
                    PointGraph::from_firdct(&self.firdata3, self.vw.zoom, sz, self.test);
                // self.mean_pings = PointGraph::from_firdctdbg(&self.firdata3);
                self.stddevup_pings =
                    PointGraph::from_firdct(&self.firdata2, self.vw.zoom, sz, self.test);
                // self.stddevdown_pings =
                //     PointGraph::from_firdct(&self.firdata, self.vw.zoom, sz, self.test);
            }
        }
    }
    pub fn update_zoom(&mut self, zoom: f64) {
        self.vw.zoom = zoom;
        self.redraw();
    }
    pub fn update_test(&mut self, test: f64) {
        self.test = test;
        self.redraw();
    }
}

impl Program<Msg> for &mut Graph {
    fn update(
        &mut self,
        _event: iced::canvas::Event,
        bounds: iced::Rectangle,
        _cursor: iced::canvas::Cursor,
    ) -> (iced::canvas::event::Status, Option<Msg>) {
        if self.size != Some(bounds.size()) {
            self.size = Some(bounds.size());
            self.redraw();
        }
        (event::Status::Ignored, None)
    }
    fn draw(
        &self,
        bounds: iced::Rectangle,
        _cursor: iced::canvas::Cursor,
    ) -> Vec<iced::canvas::Geometry> {
        let sz = bounds.size();
        let mut frame = Frame::new(sz);
        // let green_st = Stroke {
        //     width: 0.5,
        //     color: Color::from_rgba8(0, 200, 0, 0.8),
        //     ..Stroke::default()
        // };
        // let yellow_st = Stroke {
        //     width: 1.5,
        //     color: Color::from_rgba8(255, 192, 0, 0.5),
        //     ..Stroke::default()
        // };
        let orange_st = Stroke {
            width: 0.5,
            color: Color::from_rgba8(255, 64, 0, 0.9),
            ..Stroke::default()
        };
        // let black_st = Stroke {
        //     width: 1.5,
        //     color: Color::from_rgba8(0, 0, 0, 0.9),
        //     ..Stroke::default()
        // };
        let white_st = Stroke {
            width: 0.5,
            color: Color::from_rgba8(255, 255, 255, 0.8),
            ..Stroke::default()
        };

        // self.raw_pings.draw(&mut frame, green_st, &self.vw);
        self.stddevup_pings.draw(&mut frame, orange_st, &self.vw);
        // self.stddevdown_pings.draw(&mut frame, yellow_st, &self.vw);
        self.mean_pings.draw(&mut frame, white_st, &self.vw);

        vec![frame.into_geometry()]
    }
}
