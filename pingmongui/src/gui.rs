use super::graph_plot::LatencyGraph;
use super::udp_comm::UdpStats;
use iced::{canvas, executor, Application, Canvas, Column, Command, Element, Length, Subscription};
use std::net::UdpSocket;
use std::time::Instant;

#[derive(Default)]
pub struct PingmonGUI {
    pub graph: LatencyGraph,
    pub graph_canvas: canvas::layer::Cache<LatencyGraph>,
    pub socket: Option<UdpSocket>,
}

impl PingmonGUI {
    fn startup(&mut self) {
        let socket = UdpSocket::bind("127.0.0.1:7879").unwrap();
        socket.set_nonblocking(true).unwrap();
        socket.connect("127.0.0.1:7878").unwrap();

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

#[derive(Debug, Clone, Copy)]
pub enum Message {
    Tick(Instant),
    Startup,
}

impl Application for PingmonGUI {
    type Message = Message;
    type Executor = executor::Default;
    type Flags = ();

    fn new(_flags: ()) -> (Self, Command<Message>) {
        (Self::default(), Command::from(async { Message::Startup }))
    }

    fn title(&self) -> String {
        String::from("Ping Monitor")
    }

    fn subscription(&self) -> Subscription<Message> {
        super::subscr_time::every(std::time::Duration::from_millis(200)).map(Message::Tick)
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
