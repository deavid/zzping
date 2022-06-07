use crate::{basicgraph, flags::Flags};
use anyhow::{Context, Result};
use iced::{Alignment, Application, Canvas, Column, Subscription, Text};
use std::{fs::File, io::BufReader, time::Instant};
use zzping_lib::{framedataq::FDCodecIter, pingdata::Ping};

#[derive(Debug, Clone, Copy)]
pub enum Msg {
    Startup,
    Tick(Instant),
}
#[derive(Debug, Default)]
pub struct Widgets {
    graph: basicgraph::Graph,
}

#[derive(Debug, Default)]
pub struct FirTest {
    flags: Flags,
    should_quit: bool,
    widgets: Widgets,
}

type Cmd = iced::Command<Msg>;
type Elem<'a> = iced::Element<'a, Msg>;

impl FirTest {
    fn startup(&mut self) -> Result<()> {
        dbg!("START UP");
        if let Some(input) = self.flags.otheropts.input_file.as_ref() {
            eprintln!("Loading file: {}", input);
            let f = File::open(input)?;
            let buf = BufReader::new(f);
            let fdreader = FDCodecIter::new(buf);
            let mut data: Vec<Ping> = vec![];
            for item in fdreader {
                let p = Ping::from_framedataq(item)
                    .context("converting input file into basic pings")?;
                if p.rtt_us > 0 {
                    data.push(p);
                }
            }
            dbg!(data.len());
            self.widgets.graph.load(data);
        }
        Ok(())
    }
    fn tick(&mut self) {}

    fn process_message(&mut self, msg: Msg) -> Result<()> {
        match msg {
            Msg::Tick(_) => self.tick(),
            Msg::Startup => self.startup().context("startup error")?,
        }
        Ok(())
    }
}

impl Application for FirTest {
    type Message = Msg;

    type Executor = iced::executor::Default;

    type Flags = Flags;

    fn title(&self) -> String {
        "FIR Test".to_string()
    }

    fn update(&mut self, msg: Msg) -> Cmd {
        if let Err(e) = self.process_message(msg) {
            eprintln!("{:#?}", e);
            eprintln!("ERROR: {}", e);
            self.should_quit = true;
        }
        Cmd::none()
    }
    fn subscription(&self) -> Subscription<Msg> {
        super::subscr_time::every(std::time::Duration::from_millis(1000)).map(Msg::Tick)
    }

    fn view(&mut self) -> Elem {
        let input = self.flags.otheropts.input_file.clone().unwrap_or_default();
        Column::new()
            .padding(5)
            .align_items(Alignment::Center)
            .push(Text::new(input).size(20).color(iced::Color::WHITE))
            .push(
                Canvas::new(&mut self.widgets.graph)
                    .height(iced::Length::Fill)
                    .width(iced::Length::Fill),
            )
            .into()
    }

    fn new(flags: Flags) -> (Self, Cmd) {
        let ret = Self {
            flags,
            ..Default::default()
        };
        let msg = Cmd::perform(async { Msg::Startup }, |x| x);

        (ret, msg)
    }

    fn background_color(&self) -> iced::Color {
        iced::Color::from_rgb(0.25, 0.25, 0.30)
    }

    fn should_exit(&self) -> bool {
        self.should_quit
    }
}
