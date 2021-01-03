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

use crate::{
    fdq_graph::FDQGraph,
    flags::{Flags, OtherOpts},
};

use super::flags::GuiConfig;
use super::graph_plot::LatencyGraph;
use super::udp_comm::UdpStats;
use iced::{
    canvas, executor, slider, Application, Canvas, Column, Command, Element, Length, Slider,
    Subscription,
};
use std::net::UdpSocket;
use std::time::Instant;

#[derive(Debug, Clone, Copy)]
pub enum Message {
    ZoomXSliderChanged(f32),
    PosXSliderChanged(f32),
    Tick(Instant),
    Startup,
}

#[derive(Default)]
pub struct PingmonGUI {
    pub guiconfig: GuiConfig,
    pub otheropts: OtherOpts,
    pub graph: LatencyGraph,
    pub graph_canvas: canvas::layer::Cache<LatencyGraph>,
    pub socket: Option<UdpSocket>,
    pub fdqgraph: FDQGraph,
    pub fdqgraph_canvas: canvas::layer::Cache<FDQGraph>,
    zoomx_slider_state: slider::State,
    zoomx_slider: f32,
    posx_slider_state: slider::State,
    posx_slider: f32,
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
            if self.graph.update(instant, stats) {
                self.graph_canvas.clear();
            }
        } else if self.fdqgraph.update(instant) {
            self.fdqgraph_canvas.clear();
        }
    }
}

impl Application for PingmonGUI {
    type Message = Message;
    type Executor = executor::Default;
    type Flags = Flags;

    fn new(flags: Flags) -> (Self, Command<Message>) {
        let app = Self {
            graph: LatencyGraph::new(&flags.guiconfig.display_address),
            guiconfig: flags.guiconfig,
            otheropts: flags.otheropts,
            ..Self::default()
        };
        (app, Command::from(async { Message::Startup }))
    }

    fn title(&self) -> String {
        match self.otheropts.input_file.as_ref() {
            Some(input) => format!("Ping Monitor - File: {}", input),
            None => "Ping Monitor".to_owned(),
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        super::subscr_time::every(std::time::Duration::from_millis(50)).map(Message::Tick)
    }

    fn update(&mut self, message: Message) -> Command<Message> {
        match message {
            Message::ZoomXSliderChanged(x) => {
                self.zoomx_slider = x;
                self.fdqgraph.set_zoomx(x.exp() as f64);
            }
            Message::PosXSliderChanged(x) => {
                self.posx_slider = x;
                self.fdqgraph.set_posx(x as f64);
            }
            Message::Tick(instant) => self.tick(instant),
            Message::Startup => self.startup(),
        };
        Command::none()
    }

    fn view(&mut self) -> Element<Message> {
        let mut window = Column::new().padding(0);
        if self.otheropts.input_file.is_none() {
            let graph = Canvas::new()
                .width(Length::Fill)
                .height(Length::Fill)
                .push(self.graph_canvas.with(&self.graph));

            window = window.push(graph);
        } else {
            let graph = Canvas::new()
                .width(Length::Fill)
                .height(Length::Fill)
                .push(self.fdqgraph_canvas.with(&self.fdqgraph));
            window = window.push(graph);
            window = window.push(Slider::new(
                &mut self.zoomx_slider_state,
                0.0..=6.0,
                self.zoomx_slider,
                Message::ZoomXSliderChanged,
            ));
            window = window.push(Slider::new(
                &mut self.posx_slider_state,
                0.0..=1.0,
                self.posx_slider,
                Message::PosXSliderChanged,
            ));
        }
        window.into()
    }
}
