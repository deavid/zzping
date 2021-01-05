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

mod config;
mod icmp;
mod transport;

use chrono::{DateTime, Utc};
use std::net::UdpSocket;
use std::time::{Duration, Instant};

#[macro_use]
extern crate log;
extern crate env_logger;
extern crate rmp;
extern crate zzpinglib;

use clap::Clap;
use zzpinglib::framedata::{FrameData, FrameTime};
use zzpinglib::framestats::FrameStats;

#[derive(Clap)]
#[clap(
    version = "0.2.0-beta1",
    author = "David Martinez Marti <deavidsedice@gmail.com>"
)]
struct Opts {
    #[clap(short, long, default_value = "daemon_config.ron")]
    config: String,
}

fn clearscreen() {
    print!("{esc}[2J{esc}[1;1H", esc = 27 as char);
}
fn main() {
    let opts: Opts = Opts::parse();
    let cfg = config::ServerConfig::from_file(&opts.config).unwrap();
    let socket = UdpSocket::bind(&cfg.udp_listen_address).unwrap();
    socket.set_nonblocking(true).unwrap();

    let interval = Duration::from_millis(5);
    let wait = Duration::from_millis(5);
    let refresh = Duration::from_millis(100);

    let pckt_loss_inflight_time = Duration::from_millis(150);
    let pckt_loss_recv_time = Duration::from_millis(300);
    let time_avg = Duration::from_millis(200);
    let mut last_refresh = Instant::now() - Duration::from_secs(60);
    let mut t = transport::Comms::new(transport::CommConfig {
        forget_lost: Duration::from_millis(5000),
        forget_inflight: Duration::from_millis(5000),
        forget_recv: Duration::from_millis(5000),
    });
    let mut time_since_report = Instant::now() - Duration::from_secs(60);
    let now: DateTime<Utc> = Utc::now();
    let mut strnow = now
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
        .replace("-", "")
        .replace(":", "");
    strnow.truncate(11);

    for target in cfg.ping_targets {
        t.add_destination(&target, interval);
    }
    for dest in t.dest.iter_mut() {
        dest.create_log_file(&strnow);
    }
    loop {
        t.send_all();
        t.recv_all(wait);

        let elapsed = last_refresh.elapsed();
        if elapsed > refresh {
            last_refresh = Instant::now();
            t.cleanup();
            clearscreen();
            let since_report_elapsed = time_since_report.elapsed();
            if since_report_elapsed > Duration::from_secs(15) {
                time_since_report = Instant::now();
                let mut newstrnow = Utc::now()
                    .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
                    .replace("-", "")
                    .replace(":", "");
                newstrnow.truncate(11);
                if newstrnow != strnow {
                    strnow = newstrnow;
                    for dest in t.dest.iter_mut() {
                        dest.create_log_file(&strnow);
                    }
                }
            }
            let mut udp_ok = true;
            for dest in t.dest.iter() {
                let inflight_count = dest.inflight_packets.len();
                let recv_count = dest.recv_packets.len();
                let packets_lost = dest.inflight_packets.iter().fold(0, |acc, x| {
                    acc + if last_refresh - x.sent >= pckt_loss_inflight_time {
                        1
                    } else {
                        0
                    }
                }) + dest.lost_packets.len();
                let packets_recv = dest.recv_packets.iter().fold(0, |acc, x| {
                    acc + if last_refresh - (x.sent + x.received.unwrap()) <= pckt_loss_recv_time {
                        1
                    } else {
                        0
                    }
                });
                let packet_loss =
                    (100.0 * packets_lost as f32) / ((packets_lost + packets_recv) as f32);
                let avg = dest
                    .recv_packets
                    .iter()
                    .filter(|x| (x.sent + x.received.unwrap()).elapsed() < time_avg);
                let tot_time: Duration = avg.clone().fold(Duration::from_micros(0), |acc, x| {
                    acc + x.received.unwrap_or_default()
                });
                let avg_len = avg.count().max(1);
                let avg_time: Duration = if dest.recv_packets.is_empty() {
                    Duration::from_millis(999)
                } else {
                    tot_time / (avg_len as u32)
                };
                let last_pckt_received = dest
                    .recv_packets
                    .last()
                    .map_or(last_refresh, |x| x.sent)
                    .elapsed();
                let recv_per_sec = recv_count as f32 / t.config.forget_recv.as_secs_f32();
                println!(
                    "{:>14?} - {:>4} in-flight - {:>4.2} recv/s - {:>7.2?}ms / {:>4.1?}s - {:>7.2}% loss ({}/{}) ident: {},{}",
                    dest.addr,
                    inflight_count,
                    recv_per_sec,
                    avg_time.as_secs_f32() * 1000.0,
                    last_pckt_received.as_secs_f32(),
                    packet_loss,
                    packets_lost,
                    packets_recv,
                    dest.ident,
                    dest.seq,
                );
                match FrameStats::encode_stats(
                    dest.addr,
                    inflight_count,
                    avg_time,
                    last_pckt_received,
                    packet_loss,
                ) {
                    Ok(msg) => {
                        udp_ok = udp_ok && socket.send_to(&msg, &cfg.udp_client_address).is_ok()
                    }
                    Err(e) => println!("UDP Encode error: {}", e),
                }
            }
            if !udp_ok {
                println!("Error sending via UDP. Client might not be connected.")
            }
            for dest in t.dest.iter_mut() {
                let last_recv = dest.received_last(refresh * 2);
                let inflight = dest.inflight_after(refresh);
                let mut last_recv_us: Vec<u128> = last_recv
                    .iter()
                    .map(|p| p.received.unwrap_or_default().as_micros())
                    .collect();
                last_recv_us.sort_unstable();
                if let Some(mut f) = dest.logfile.as_mut() {
                    // TODO: Extract this as a function!
                    let time: FrameTime = if since_report_elapsed > Duration::from_secs(15) {
                        FrameTime::Timestamp(Utc::now())
                    } else {
                        FrameTime::Elapsed(since_report_elapsed)
                    };
                    let framedata = FrameData {
                        time,
                        inflight: inflight.len(),
                        lost_packets: dest.lost_packets.len(),
                        recv_us: last_recv_us,
                    };
                    if let Err(e) = framedata.encode(&mut f) {
                        println!("Error writing to file: {:?}", e);
                    }
                }
            }
        }
    }
}
