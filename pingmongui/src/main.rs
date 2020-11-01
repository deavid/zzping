use iced::{
    canvas, executor, slider, Application, Canvas, Color, Column, Command, Element, Length, Point,
    ProgressBar, Settings, Slider, Subscription,
};
use rand::Rng;
use std::net::UdpSocket;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
struct UnexpectedError {
    t: String,
}

impl UnexpectedError {
    fn new(t: &str) -> Self {
        Self { t: t.to_owned() }
    }
}

impl std::fmt::Display for UnexpectedError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "unexpected: {}", self.t)
    }
}

impl std::error::Error for UnexpectedError {}

struct UdpStats {
    pub addr: String,
    pub inflight_count: u16,
    pub avg_time_us: u32,
    pub last_pckt_ms: u32,
    pub packet_loss_x1000: u32,
}

impl UdpStats {
    fn from_buf(mut v: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        let addr: String;
        let inflight_count: u16;
        let avg_time_us: u32;
        let last_pckt_ms: u32;
        let packet_loss_x1000: u32;

        let len = rmp::decode::read_array_len(&mut v)?;
        if len != 5 {
            return Err(Box::new(UnexpectedError::new("Array must be length 5")));
        }
        let mut buf: Vec<u8> = vec![0; 65536];
        addr = rmp::decode::read_str(&mut v, &mut buf)
            .map_err(|_| Box::new(UnexpectedError::new("Couldn't read string")))?
            .to_owned();

        inflight_count = rmp::decode::read_u16(&mut v)?;
        avg_time_us = rmp::decode::read_u32(&mut v)?;
        last_pckt_ms = rmp::decode::read_u32(&mut v)?;
        packet_loss_x1000 = rmp::decode::read_u32(&mut v)?;

        Ok(Self {
            addr,
            inflight_count,
            avg_time_us,
            last_pckt_ms,
            packet_loss_x1000,
        })
    }
}

pub fn main() {
    PingmonGUI::run(Settings {
        antialiasing: true,
        ..Settings::default()
    })
}

#[derive(Default)]
struct PingmonGUI {
    value: f32,
    progress_bar_slider: slider::State,
    graph: LatencyGraph,
    graph_canvas: canvas::layer::Cache<LatencyGraph>,
    socket: Option<UdpSocket>,
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

#[derive(Debug)]
struct LatencyGraph {
    points: Vec<u32>,
    current: Instant,
}

fn _rand_num() -> u32 {
    let mut rng = rand::thread_rng();
    rng.gen_range(1000, 2000)
        + rng.gen_range(1000, 2000)
        + rng.gen_range(1000, 2000)
        + rng.gen_range(1000, 2000)
        + rng.gen_range(1000, 2000)
}

impl LatencyGraph {
    fn new() -> Self {
        Self {
            points: vec![],
            current: Instant::now(),
        }
    }
    pub fn update(&mut self, now: Instant, stats: Vec<UdpStats>) -> bool {
        let mut modified = false;
        for s in stats {
            if s.addr == "192.168.0.1" {
                self.points.push(s.avg_time_us);
                modified = true;
            }
        }
        while self.points.len() >= 200 {
            self.points.remove(0);
            modified = true;
        }
        // self.points.push(rand_num());
        if self.current.elapsed() > Duration::from_secs(1) {
            modified = true;
        }
        if modified {
            self.current = now;
        }
        modified
    }
}

impl Default for LatencyGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl canvas::Drawable for LatencyGraph {
    fn draw(&self, frame: &mut canvas::Frame) {
        use canvas::{Path, Stroke};
        let space = Path::rectangle(Point::new(0.0, 0.0), frame.size());
        let botright = Point::new(frame.width(), frame.height());
        let bottom = frame.height();
        let green_stroke = Stroke {
            width: 3.0,
            color: Color::from_rgba8(0, 255, 0, 0.3),
            ..Stroke::default()
        };
        let red_stroke = Stroke {
            width: 3.0,
            color: Color::from_rgba8(255, 0, 0, 0.5),
            ..Stroke::default()
        };

        frame.fill(&space, Color::from_rgba8(100, 100, 100, 1.0));
        if self.points.is_empty() {
            let line = canvas::Path::line(Point::new(0.0, 0.0), botright);
            frame.stroke(&line, red_stroke);
            return;
        }
        let max = self.points.iter().max().unwrap();
        let len = self.points.len();
        let sx = frame.width() / len as f32;
        let sy = (frame.height() / *max as f32) * 0.8;
        let mut oldp: Option<Point> = None;
        for (n, p) in self.points.iter().enumerate() {
            let x = n as f32 * sx;
            let y = bottom - *p as f32 * sy;
            let point = Point::new(x, y);
            if let Some(oldp) = oldp {
                let line = canvas::Path::line(oldp, point);
                frame.stroke(&line, green_stroke);
            }
            if n == len - 1 {
                let x2 = frame.width();
                let p2 = Point::new(x2, y);
                let line = canvas::Path::line(point, p2);
                frame.stroke(&line, green_stroke);
            }
            oldp = Some(point);
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Message {
    SliderChanged(f32),
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
        time::every(std::time::Duration::from_millis(10)).map(Message::Tick)
    }

    fn update(&mut self, message: Message) -> Command<Message> {
        // dbg!(message);
        match message {
            Message::SliderChanged(x) => self.value = x,
            Message::Tick(instant) => {
                // println!("Tick: {:#?}", instant);
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
        // fn Called per each frame!
        // println!("View called");
        let graph = Canvas::new()
            .width(Length::Fill)
            .height(Length::Fill)
            .push(self.graph_canvas.with(&self.graph));

        Column::new()
            .padding(10)
            .push(ProgressBar::new(0.0..=100.0, self.value))
            .push(Slider::new(
                &mut self.progress_bar_slider,
                0.0..=100.0,
                self.value,
                Message::SliderChanged,
            ))
            .push(graph)
            .into()
    }
}

mod time {
    use iced::futures;
    use std::time::Instant;

    pub fn every(duration: std::time::Duration) -> iced::Subscription<Instant> {
        iced::Subscription::from_recipe(Every(duration))
    }

    struct Every(std::time::Duration);

    impl<H, I> iced_native::subscription::Recipe<H, I> for Every
    where
        H: std::hash::Hasher,
    {
        type Output = Instant;

        fn hash(&self, state: &mut H) {
            use std::hash::Hash;

            std::any::TypeId::of::<Self>().hash(state);
            self.0.hash(state);
        }

        fn stream(
            self: Box<Self>,
            _input: futures::stream::BoxStream<'static, I>,
        ) -> futures::stream::BoxStream<'static, Self::Output> {
            use futures::stream::StreamExt;

            async_std::stream::interval(self.0)
                .map(|_| Instant::now())
                .boxed()
        }
    }
}
