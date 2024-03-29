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

use crate::{
    fdq_graph::FDQGraph,
    flags::{Flags, OtherOpts},
};

use super::flags::GuiConfig;
use super::graph_plot::LatencyGraph;
use super::udp_comm::UdpStats;
use iced::{
    executor, slider, Application, Canvas, Color, Column, Command, Element, Length, Row, Slider,
    Subscription, Text,
};
use std::net::UdpSocket;
use std::time::Instant;

#[derive(Debug, Clone, Copy)]
pub enum Message {
    ZoomWSliderChanged(f32),
    ZoomYSliderChanged(f32),
    ZoomXSliderChanged(f32),
    // PosXSliderChanged(f32),
    PosDXSliderChanged(f32),
    Tick(Instant),
    Startup,
}

pub struct PingmonGUI {
    pub display_address: Vec<String>,
    pub guiconfig: GuiConfig,
    pub otheropts: OtherOpts,
    pub graph: Vec<LatencyGraph>,
    pub graph_cache: Vec<iced::widget::canvas::Cache>,
    pub socket: Option<UdpSocket>,
    pub fdqgraph: FDQGraph,
    pub fdqgraph_cache: iced::widget::canvas::Cache,
    zoomw_slider_state: slider::State,
    zoomw_slider: f32,
    zoomy_slider_state: slider::State,
    zoomy_slider: f32,
    zoomx_slider_state: slider::State,
    zoomx_slider: f32,
    // posx_slider_state: slider::State,
    posx_slider: f32,
    posdx_slider_state: slider::State,
    posdx_slider: f32,
}

impl Default for PingmonGUI {
    fn default() -> Self {
        Self {
            posx_slider: 0.5,
            display_address: vec![],
            guiconfig: Default::default(),
            otheropts: Default::default(),
            graph: Default::default(),
            graph_cache: Default::default(),
            socket: Default::default(),
            fdqgraph: Default::default(),
            fdqgraph_cache: Default::default(),
            zoomw_slider_state: Default::default(),
            zoomw_slider: Default::default(),
            zoomy_slider_state: Default::default(),
            zoomy_slider: Default::default(),
            zoomx_slider_state: Default::default(),
            zoomx_slider: Default::default(),
            // posx_slider_state: Default::default(),
            posdx_slider_state: Default::default(),
            posdx_slider: Default::default(),
        }
    }
}
impl PingmonGUI {
    fn startup(&mut self) {
        let input_file = self.otheropts.input_file.as_ref();
        match input_file {
            Some(filename) => self.fdqgraph.load_file(filename),
            None => {
                let socket = UdpSocket::bind(&self.guiconfig.udp_listen_address).unwrap();
                socket.set_nonblocking(true).unwrap();
                socket.connect(&self.guiconfig.udp_server_address).unwrap();

                self.socket = Some(socket);
            }
        }
    }
    fn recv(&mut self) -> Result<UdpStats, Box<dyn std::error::Error>> {
        let mut buf: [u8; 65536] = [0; 65536];
        let socket = self.socket.as_mut().unwrap();
        let sz = socket.recv(&mut buf)?;
        let stats = UdpStats::from_buf(&buf[..sz])?;
        Ok(stats)
    }
    fn recv_all(&mut self) -> Vec<UdpStats> {
        let mut ret: Vec<UdpStats> = vec![];
        while let Ok(stats) = self.recv() {
            ret.push(stats);
        }
        ret
    }
    fn tick(&mut self, instant: Instant) {
        if self.otheropts.input_file.is_none() {
            let stats = self.recv_all();
            for (graph, canvas) in self.graph.iter_mut().zip(self.graph_cache.iter_mut()) {
                if graph.update(instant, &stats) {
                    canvas.clear();
                }
            }
        } else {
            if self.fdqgraph.update(instant) {
                self.fdqgraph_cache.clear();
            }
            if self.posdx_slider.abs() > 0.01 {
                let adx = self.posdx_slider.signum() / 500.0;
                let z = (self.zoomx_slider as f64).exp();
                let dx = self.posdx_slider as f64 / z;
                let factor = 1.0 / 100.0;
                self.posx_slider += dx as f32 * factor;
                self.posdx_slider -= adx;
                if self.posdx_slider.abs() < 0.01 {
                    self.posdx_slider = 0.0;
                }
                self.posx_slider = self.posx_slider.max(0.0).min(1.0);
                self.update_posx();
            }
        }
    }
    fn update_posx(&mut self) {
        let x = self.posx_slider as f64;
        // let z = (self.zoomx_slider as f64).exp();
        // let dx = self.posdx_slider as f64 / z;
        // let fx = x;
        self.fdqgraph.set_posx(x.max(0.0).min(1.0));
    }
}

impl Application for PingmonGUI {
    type Message = Message;
    type Executor = executor::Default;
    type Flags = Flags;

    fn new(flags: Flags) -> (Self, Command<Message>) {
        let app = Self {
            display_address: flags.guiconfig.display_address.clone(),
            graph: flags
                .guiconfig
                .display_address
                .iter()
                .map(|addr| LatencyGraph::new(addr, flags.guiconfig.sample_limit))
                .collect(),
            graph_cache: flags
                .guiconfig
                .display_address
                .iter()
                .map(|_| Default::default())
                .collect(),

            guiconfig: flags.guiconfig,
            otheropts: flags.otheropts,
            ..Self::default()
        };
        (app, Command::perform(async { Message::Startup }, |x| x))
    }

    fn title(&self) -> String {
        match self.otheropts.input_file.as_ref() {
            Some(input) => format!("Ping Monitor - File: {}", input),
            None => "Ping Monitor".to_owned(),
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        super::subscr_time::every(std::time::Duration::from_millis(20)).map(Message::Tick)
    }

    fn update(&mut self, message: Message) -> Command<Message> {
        match message {
            Message::ZoomWSliderChanged(w) => {
                self.zoomw_slider = w;
                self.fdqgraph.set_scalefactor((-w).exp() as f64);
            }
            Message::ZoomYSliderChanged(y) => {
                self.zoomy_slider = y;
                self.fdqgraph.set_zoomy(y.exp() as f64);
            }
            Message::ZoomXSliderChanged(x) => {
                self.zoomx_slider = x;
                self.fdqgraph.set_zoomx(x.exp() as f64);
            }
            // Message::PosXSliderChanged(x) => {
            //     self.posx_slider = x;
            //     self.update_posx();
            // }
            Message::PosDXSliderChanged(x) => {
                self.posdx_slider = x;
                self.update_posx();
            }
            Message::Tick(instant) => self.tick(instant),
            Message::Startup => self.startup(),
        };
        Command::none()
    }

    fn view(&mut self) -> Element<Message> {
        let mut window: Column<Message> = Column::new().padding(0);
        if self.otheropts.input_file.is_none() {
            for (_addr, (graph, _cache)) in self
                .display_address
                .iter()
                .zip(self.graph.iter().zip(self.graph_cache.iter()))
            {
                // FIXME: This clones the graph data AND doesn't use the Cache!
                // let widget_graph = Canvas::new(cache.with(graph))
                //     .width(Length::Fill)
                //     .height(Length::Fill);
                let widget_graph = Canvas::new(graph.clone())
                    .width(Length::Fill)
                    .height(Length::Fill);
                window = window.push(widget_graph);
            }
        } else {
            // FIXME: This clones the graph data AND doesn't use the Cache!
            let graph = Canvas::new(self.fdqgraph.clone())
                .width(Length::Fill)
                .height(Length::Fill);

            // let graph = Canvas::new(self.fdqgraph_cache.with(&self.fdqgraph))
            //     .width(Length::Fill)
            //     .height(Length::Fill);
            window = window.push(graph);
            let mut row2: Row<Message> = Row::new().padding(4).spacing(5);
            row2 = row2.push(Text::new("sf").size(20).color(Color::BLACK));
            row2 = row2.push(
                Slider::new(
                    &mut self.zoomw_slider_state,
                    -2.0..=2.0,
                    self.zoomw_slider,
                    Message::ZoomWSliderChanged,
                )
                .step(0.01),
            );
            row2 = row2.push(Text::new("y").size(20).color(Color::BLACK));
            row2 = row2.push(
                Slider::new(
                    &mut self.zoomy_slider_state,
                    0.0..=8.0,
                    self.zoomy_slider,
                    Message::ZoomYSliderChanged,
                )
                .step(0.01),
            );
            row2 = row2.push(Text::new("z").size(20).color(Color::BLACK));
            row2 = row2.push(
                Slider::new(
                    &mut self.zoomx_slider_state,
                    0.0..=10.0,
                    self.zoomx_slider,
                    Message::ZoomXSliderChanged,
                )
                .step(0.01),
            );
            // row2 = row2.push(Text::new("x").size(20).color(Color::BLACK));
            // row2 = row2.push(Slider::new(
            //     &mut self.posx_slider_state,
            //     0.0..=1.0,
            //     self.posx_slider,
            //     Message::PosXSliderChanged,
            // ));
            row2 = row2.push(Text::new("dx").size(20).color(Color::BLACK));
            row2 = row2.push(
                Slider::new(
                    &mut self.posdx_slider_state,
                    -1.0..=1.0,
                    self.posdx_slider,
                    Message::PosDXSliderChanged,
                )
                .step(0.01),
            );

            window = window.push(row2);
        }
        window.into()
    }
}
