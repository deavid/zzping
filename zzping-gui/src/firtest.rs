use crate::{basicgraph, flags::Flags};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use iced::{Alignment, Subscription, Element, Length, Task};
use iced::widget::{Canvas, Column, Slider, Text};
use log::{info, warn};
use std::{collections::VecDeque, fs::File, io::BufReader, time::Instant};
use zzping_lib::{
    framedataq::FDCodecIter,
    pingdata::{Ping, StreamData, StreamEventType, StreamPingIO},
};

#[derive(Debug, Clone, Copy)]
pub enum Msg {
    Startup,
    Tick(Instant),
    ZoomChange(f64),
    TestChange(f64),
}

#[derive(Debug, Default)]
pub struct Widgets {
    ping_graph: basicgraph::Graph,
    loss_graph: basicgraph::Graph,
    zoom: f64,
    test: f64,
}

#[derive(Debug, Default)]
pub struct FirTest {
    flags: Flags,
    should_quit: bool,
    widgets: Widgets,
}

impl FirTest {
    fn startup(&mut self) -> Result<()> {
        dbg!("START UP");
        if let Some(input) = self.flags.otheropts.input_file.as_ref() {
            info!("Loading file: {}", input);
            let f = File::open(input)?;
            let mut buf = BufReader::new(f);
            let mut data: Vec<Ping> = vec![];
            let mut loss: Vec<Ping> = vec![];
            if input.contains("-streamdata") {
                info!("Loading streamdata");
                let sd = StreamData::from_file_header(&mut buf).context("SD:header")?;
                info!("{:?}", sd);
                let max_ping = sd.pending_pings_us.first().copied().unwrap_or_default();
                // FIXME: Use StreamPingIO::from_file_header instead!
                let mut spr = StreamPingIO::from_header(sd);
                let mut pending: VecDeque<u64> = spr
                    .header
                    .pending_pings_us
                    .iter()
                    .copied()
                    .map(|x| x - max_ping)
                    .collect();

                let init_time: DateTime<Utc> = spr.header.initial_time;
                let mut cur_time = init_time;
                while let Some(sp) = spr.read_next(&mut buf).context("SDR:read")? {
                    cur_time = init_time + chrono::Duration::microseconds(sp.delta as i64);
                    match sp.event {
                        StreamEventType::Received => {
                            if pending.is_empty() {
                                warn!("{}us - no ping sent?", sp.delta);
                                continue;
                            }
                            let idx = sp.scode.min(pending.len() as u8 - 1);
                            if idx != sp.scode {
                                warn!(
                                    "ping sent code: {} but queue has size {}",
                                    sp.scode,
                                    pending.len()
                                );
                            }
                            let sent = pending.remove(idx as usize).unwrap();
                            let rtt_us = (sp.delta - sent) as i64;
                            // if sp.scode != 0 {
                            //     info!("{}us - {} left code:{}", rtt_us, pending.len(), sp.scode);
                            // }
                            let sent_time = init_time + chrono::Duration::microseconds(sent as i64);

                            data.push(Ping {
                                // Reporting from sent_time instead from receiving time seems to be better.
                                // The problem is that this metric might be "in the past" of a block.
                                received: sent_time,
                                rtt_us,
                            });
                        }
                        StreamEventType::Sent => {
                            if sp.scode > 0 {
                                if pending.is_empty() {
                                    warn!("packet lost: {} - queue is empty", sp.scode);
                                    continue;
                                }
                                let idx = if sp.scode == 255 { 0 } else { sp.scode - 1 };
                                let idx = idx.min(pending.len() as u8 - 1);
                                let sent = pending.remove(idx as usize).unwrap();
                                let sent_time =
                                    init_time + chrono::Duration::microseconds(sent as i64);
                                for p in loss.iter_mut().rev() {
                                    if p.received <= sent_time && p.rtt_us > 500_000 {
                                        p.rtt_us = 500_000;
                                        break;
                                    }
                                }
                            } else {
                                pending.push_back(sp.delta);
                                loss.push(Ping {
                                    received: cur_time,
                                    rtt_us: 1_000_000,
                                });
                            }
                        }
                    }
                }
                info!(
                    "Last timestamp seen: {}",
                    chrono::DateTime::<chrono::Local>::from(cur_time)
                );
            } else {
                info!("Loading framedataq");
                let fdreader = FDCodecIter::new(buf);
                for item in fdreader {
                    let p = Ping::from_framedataq(item)
                        .context("converting input file into basic pings")?;
                    if p.rtt_us > 0 {
                        data.push(p);
                    }
                }
            }
            dbg!(data.len());
            self.widgets.ping_graph.load(data);
            // self.widgets.loss_graph.load(loss);
        }
        Ok(())
    }
    fn tick(&mut self) {}

    fn process_message(&mut self, msg: Msg) -> Result<()> {
        match msg {
            Msg::TestChange(x) => self.on_test_change(x),
            Msg::ZoomChange(x) => self.on_zoom_change(x),
            Msg::Tick(_) => self.tick(),
            Msg::Startup => self.startup().context("startup error")?,
        }
        Ok(())
    }
    pub fn on_zoom_change(&mut self, pos: f64) {
        self.widgets.zoom = pos;
        let zoom = f64::exp(pos / 100.0);
        self.widgets.ping_graph.update_zoom(zoom);
        self.widgets.loss_graph.update_zoom(zoom);
    }

    pub fn on_test_change(&mut self, pos: f64) {
        self.widgets.test = pos;
        let test = (pos / 1000.0).powi(10);
        dbg!((1.0 + 2.0 / test).log2());
        self.widgets.ping_graph.update_test(test);
    }
}
impl FirTest {
    pub fn new(flags: Flags) -> (Self, Task<Msg>) {
        let ret = Self {
            flags,
            ..Default::default()
        };
        let task = Task::perform(async { Msg::Startup }, |x| x);
        (ret, task)
    }

    pub fn title(&self) -> String {
        "FIR Test".to_string()
    }

    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        if let Err(e) = self.process_message(msg) {
            eprintln!("{:#?}", e);
            eprintln!("ERROR: {}", e);
            self.should_quit = true;
        }
        Task::none()
    }
    
    pub fn subscription(&self) -> Subscription<Msg> {
        iced::time::every(std::time::Duration::from_millis(1000)).map(Msg::Tick)
    }

    pub fn view(&self) -> Element<'_, Msg> {
        let input = self.flags.otheropts.input_file.clone().unwrap_or_default();
        Column::new()
            .padding(5)
            .align_x(Alignment::Center)
            .push(Text::new(input).size(20).color(iced::Color::WHITE))
            .push(
                Canvas::new(&self.widgets.ping_graph)
                    .height(Length::Fill)
                    .width(Length::Fill),
            )
            .push(Slider::new(
                0.0..=1000.0,
                self.widgets.zoom,
                Msg::ZoomChange,
            ))
            .push(Slider::new(
                0.0..=1000.0,
                self.widgets.test,
                Msg::TestChange,
            ))
            .into()
    }

    pub fn background_color(&self) -> iced::Color {
        iced::Color::from_rgb(0.25, 0.25, 0.30)
    }

    pub fn should_exit(&self) -> bool {
        self.should_quit
    }
}
