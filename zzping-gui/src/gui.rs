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
use iced::widget::{
    button, canvas, container, Canvas, Column, Container, Row, Slider, Stack, Text,
};
use iced::{Element, Length, Subscription, Task};
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
    #[allow(dead_code)]
    Startup,
}

pub struct PingmonGUI {
    pub guiconfig: GuiConfig,
    pub otheropts: OtherOpts,
    pub graph: Vec<LatencyGraph>,
    pub socket: Option<UdpSocket>,
    pub fdqgraph: FDQGraph,
    // pub fdqgraph_cache: iced::widget::canvas::Cache,
    zoomw_slider: f32,
    zoomy_slider: f32,
    zoomx_slider: f32,
    posx_slider: f32,
    posdx_slider: f32,
}

impl Default for PingmonGUI {
    fn default() -> Self {
        Self {
            posx_slider: 0.5,
            guiconfig: Default::default(),
            otheropts: Default::default(),
            graph: Default::default(),
            socket: Default::default(),
            fdqgraph: Default::default(),
            zoomw_slider: Default::default(),
            zoomy_slider: Default::default(),
            zoomx_slider: Default::default(),
            posdx_slider: Default::default(),
        }
    }
}
impl PingmonGUI {
    fn startup(&mut self) {
        let input_file = self.otheropts.input_file.as_ref();
        match input_file {
            Some(_filename) => { /* self.fdqgraph.load_file(filename) */ }
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
            for graph in self.graph.iter_mut() {
                graph.update(instant, &stats);
            }
        } else {
            // if self.fdqgraph.update(instant) {
            //     self.fdqgraph_cache.clear();
            // }
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
                self.posx_slider = self.posx_slider.clamp(0.0, 1.0);
                self.update_posx();
            }
        }
    }
    fn update_posx(&mut self) {
        let _x = self.posx_slider as f64;
        // let z = (self.zoomx_slider as f64).exp();
        // let dx = self.posdx_slider as f64 / z;
        // let fx = x;
        // self.fdqgraph.set_posx(x.clamp(0.0, 1.0));
    }
}

impl PingmonGUI {
    pub fn new(flags: Flags) -> Self {
        let mut app = Self {
            graph: flags
                .guiconfig
                .display_address
                .iter()
                .map(|addr| LatencyGraph::new(addr, flags.guiconfig.sample_limit))
                .collect(),

            guiconfig: flags.guiconfig,
            otheropts: flags.otheropts,
            ..Self::default()
        };
        app.startup();
        app
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ZoomWSliderChanged(w) => {
                self.zoomw_slider = w;
                // self.fdqgraph.set_scalefactor((-w).exp() as f64);
            }
            Message::ZoomYSliderChanged(y) => {
                self.zoomy_slider = y;
                // self.fdqgraph.set_zoomy(y.exp() as f64);
            }
            Message::ZoomXSliderChanged(x) => {
                self.zoomx_slider = x;
                // self.fdqgraph.set_zoomx(x.exp() as f64);
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
            Message::Startup => { /* Handled in new() now */ }
        };
        Task::none()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        iced::time::every(std::time::Duration::from_millis(50)).map(Message::Tick)
    }

    pub fn view(&self) -> Element<'_, Message> {
        let mut content = Column::new().spacing(5).padding(1).clip(false);
        if self.otheropts.input_file.is_none() {
            for graph in self.graph.iter() {
                // Use Canvas with reference instead of clone in iced 0.13
                let widget_graph = Canvas::new(graph)
                    .width(Length::Fill)
                    .height(Length::Fixed(160.0));
                // let container = Stack::new()
                //     .push(widget_graph)
                //     .push(Stack::new().height(Length::Fixed(160.0)));
                content = content.push(widget_graph).width(Length::Fill);
            }
        } else {
            // FDQ Graph - re-enabled with iced 0.13 placeholder implementation
            let graph = Canvas::new(&self.fdqgraph)
                .width(Length::Fill)
                .height(Length::Fill);

            let controls = Row::new()
                .padding(4)
                .spacing(5)
                .push(Text::new("sf").size(20))
                .push(
                    Slider::new(-2.0..=2.0, self.zoomw_slider, Message::ZoomWSliderChanged)
                        .step(0.01),
                )
                .push(Text::new("y").size(20))
                .push(
                    Slider::new(0.0..=8.0, self.zoomy_slider, Message::ZoomYSliderChanged)
                        .step(0.01),
                )
                .push(Text::new("z").size(20))
                .push(
                    Slider::new(0.0..=10.0, self.zoomx_slider, Message::ZoomXSliderChanged)
                        .step(0.01),
                )
                .push(Text::new("dx").size(20))
                .push(
                    Slider::new(-1.0..=1.0, self.posdx_slider, Message::PosDXSliderChanged)
                        .step(0.01),
                );

            content = content.push(graph).push(controls);
        }
        container(content).into()
    }
}
