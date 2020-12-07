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

use super::flags::GuiConfig;
use super::graph_plot::LatencyGraph;
use super::udp_comm::UdpStats;
use iced::{canvas, executor, Application, Canvas, Column, Command, Element, Length, Subscription};
use std::net::UdpSocket;
use std::time::Instant;

#[derive(Debug, Clone, Copy)]
pub enum Message {
    Tick(Instant),
    Startup,
}

#[derive(Default)]
pub struct PingmonGUI {
    pub config: GuiConfig,
    pub graph: LatencyGraph,
    pub graph_canvas: canvas::layer::Cache<LatencyGraph>,
    pub socket: Option<UdpSocket>,
}

impl PingmonGUI {
    fn startup(&mut self) {
        let socket = UdpSocket::bind(&self.config.udp_listen_address).unwrap();
        socket.set_nonblocking(true).unwrap();
        socket.connect(&self.config.udp_server_address).unwrap();

        self.socket = Some(socket);
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
}

impl Application for PingmonGUI {
    type Message = Message;
    type Executor = executor::Default;
    type Flags = GuiConfig;

    fn new(flags: GuiConfig) -> (Self, Command<Message>) {
        let app = Self {
            graph: LatencyGraph::new(&flags.display_address),
            config: flags,
            ..Self::default()
        };
        (app, Command::from(async { Message::Startup }))
    }

    fn title(&self) -> String {
        String::from("Ping Monitor")
    }

    fn subscription(&self) -> Subscription<Message> {
        super::subscr_time::every(std::time::Duration::from_millis(50)).map(Message::Tick)
    }

    fn update(&mut self, message: Message) -> Command<Message> {
        match message {
            Message::Tick(instant) => {
                let stats = self.recv_all();
                if self.graph.update(instant, stats) {
                    self.graph_canvas.clear();
                }
            }
            Message::Startup => self.startup(),
        };
        Command::none()
    }

    fn view(&mut self) -> Element<Message> {
        let graph = Canvas::new()
            .width(Length::Fill)
            .height(Length::Fill)
            .push(self.graph_canvas.with(&self.graph));

        Column::new().padding(0).push(graph).into()
    }
}
