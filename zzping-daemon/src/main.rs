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

mod config;
mod icmp;
mod transport;

use chrono::Utc;
use rand::Rng;
use std::net::UdpSocket;
use std::time::{Duration, Instant};

#[macro_use]
extern crate log;
extern crate env_logger;
extern crate rmp;
extern crate zzping_lib;

use clap::Clap;
use zzping_lib::framedata::{FrameData, FrameTime};
use zzping_lib::framestats::FrameStats;

struct CLIStats {
    dest_addr: std::net::IpAddr,
    inflight_count: usize,
    recv_per_sec: f32,
    avg_time: Duration,
    last_pckt_received: Duration,
    packet_loss: f32,
    packets_lost: usize,
    packets_recv: usize,
    dest_ident: u16,
    dest_seq: u16,
}

#[derive(Clap)]
#[clap(
    version = "0.2.2-beta1",
    author = "David Martinez Marti <deavidsedice@gmail.com>"
)]
struct Opts {
    #[clap(short, long, default_value = "daemon_config.ron")]
    config: String,
}

fn clearscreen() {
    print!("{esc}[2J{esc}[1;1H", esc = 27 as char);
}

fn read_config(filepath: &str) -> config::ServerConfig {
    match config::ServerConfig::from_filepath(filepath) {
        Ok(cfg) => cfg,
        Err(e) => {
            panic!(format!("Error parsing config file '{}': {:?}", filepath, e));
        }
    }
}

fn get_logfile_now() -> String {
    let mut strnow = Utc::now()
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
        .replace("-", "")
        .replace(":", "");
    strnow.truncate(11);
    strnow
}
fn main() {
    let mut rng = rand::thread_rng();

    let opts: Opts = Opts::parse();
    let cfg = read_config(&opts.config);
    let mut t = transport::Comms::new(transport::CommConfig {
        forget_lost: Duration::from_secs(cfg.keep_packets.lost_secs),
        forget_inflight: Duration::from_secs(cfg.keep_packets.inflight_secs),
        forget_recv: Duration::from_secs(cfg.keep_packets.recv_secs),
        precision_mult: cfg.precision_mult,
    });

    let socket = UdpSocket::bind(&cfg.udp_listen_address).unwrap();
    socket.set_nonblocking(true).unwrap();

    // How often the console UI is refreshed / how often to write a frame
    let cli_refresh = Duration::from_secs(1) / cfg.refresh_freq;

    // Time to assign if there are no packets reported.
    let default_recv_avg_no_packets = Duration::from_millis(0);

    // Config Stats for CLI
    let pckt_loss_inflight_time = Duration::from_millis(300);
    let pckt_loss_recv_time = Duration::from_millis(1000);

    // Timer to make the UI refresh every "cli_refresh"
    let mut last_refresh = Instant::now() - Duration::from_secs(60);
    // Timer to both enable the disk log to switch to a new file, and to write a complete packet every X
    let mut time_since_report = Instant::now() - Duration::from_secs(60);
    let report_every_secs = Duration::from_secs(15);

    // Timer to smooth the averages on the program load, to avoid seeing lower averages upon program start
    let program_start = Instant::now();

    // Contains the current ending of the file, changes every hour
    let mut strnow = get_logfile_now();

    for target in cfg.ping_targets {
        let interval = Duration::from_secs(1) / target.frequency;
        // Add a random amount to avoid having all targets at exactly the same time
        let interval_n =
            interval + Duration::from_nanos(rng.gen_range(0, interval.as_millis() + 1) as u64);

        t.add_destination(&target.address, interval_n);
    }
    for dest in t.dest.iter_mut() {
        dest.create_log_file(&strnow);
    }
    // Recommended wait ammount to be able to push all pings in time
    let wait = t.get_delay();

    // Amount of extra time taken in one round, to be able to correct it.
    loop {
        t.recv_all(wait);
        t.send_all(2);

        let elapsed = last_refresh.elapsed();
        if elapsed > cli_refresh {
            last_refresh = Instant::now();
            // Remove now the old packets from their queues. (Packets never received, old packets lost & received)
            t.cleanup();
            let since_report_elapsed = time_since_report.elapsed();
            if since_report_elapsed > report_every_secs {
                time_since_report = Instant::now();
                let newstrnow = get_logfile_now();
                if newstrnow != strnow {
                    strnow = newstrnow;
                    for dest in t.dest.iter_mut() {
                        dest.create_log_file(&strnow);
                    }
                }
            }
            // --- Compute stats phase ---

            // Used to estimate the size of the recv queue in seconds, avoids getting wrong values on program start
            let recv_time_size = t
                .config
                .forget_recv
                .min(program_start.elapsed())
                .as_secs_f32();

            // Vector to hold the stats found in each destination host
            let mut cli_stats: Vec<CLIStats> = vec![];

            for dest in t.dest.iter() {
                // The time used to average results is meant to contain 5 pings
                // in average, or one cli_refresh if that's bigger.
                let time_avg = (dest.interval * 5).max(cli_refresh);
                let inflight_count = dest.inflight_packets.len();
                let recv_count = dest.recv_packets.len();
                let inflight_long = dest.inflight_after(pckt_loss_inflight_time).len();
                let packets_lost = inflight_long + dest.lost_packets.len();
                let packets_recv = dest.received_last(pckt_loss_recv_time).len();
                let packet_loss =
                    (100.0 * packets_lost as f32) / ((packets_lost + packets_recv) as f32 + 0.1);
                let avg_time: Duration = dest
                    .mean_recv_time(time_avg)
                    .unwrap_or(default_recv_avg_no_packets);
                let last_pckt_received = dest
                    .recv_packets
                    .last()
                    .map_or(last_refresh, |x| x.sent)
                    .elapsed();
                let recv_per_sec = recv_count as f32 / recv_time_size;
                cli_stats.push(CLIStats {
                    dest_addr: dest.addr,
                    inflight_count,
                    recv_per_sec,
                    avg_time,
                    last_pckt_received,
                    packet_loss,
                    packets_lost,
                    packets_recv,
                    dest_ident: dest.ident,
                    dest_seq: dest.seq,
                });
            }
            // --- Send stats to GUI via UDP ---
            let mut udp_ok = true;
            for st in cli_stats.iter() {
                match FrameStats::encode_stats(
                    st.dest_addr,
                    st.inflight_count,
                    st.avg_time,
                    st.last_pckt_received,
                    st.packet_loss,
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
            // -- Logging phase ---
            for dest in t.dest.iter_mut() {
                let last_recv = dest.received_last(cli_refresh + cli_refresh / 2);
                let inflight = dest.inflight_after(cli_refresh);
                let mut last_recv_us: Vec<u128> = last_recv
                    .iter()
                    .map(|p| p.received.unwrap_or_default().as_micros())
                    .collect();
                last_recv_us.sort_unstable();
                if let Some(mut f) = dest.logfile.as_mut() {
                    let time: FrameTime = if since_report_elapsed > report_every_secs {
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
            // --- CLI Stats display phase ---
            // All printing behavior is sent to the end to avoid delays that cause flickering
            clearscreen();

            for st in cli_stats.iter() {
                println!(
                    "{:>14?} - {:>4} in-flight - {:>4.2} recv/s - {:>7.2?}ms / {:>4.1?}s - {:>7.2}% loss ({}/{}) ident: {},{}",
                    st.dest_addr,
                    st.inflight_count,
                    st.recv_per_sec,
                    st.avg_time.as_secs_f32() * 1000.0,
                    st.last_pckt_received.as_secs_f32(),
                    st.packet_loss,
                    st.packets_lost,
                    st.packets_recv,
                    st.dest_ident,
                    st.dest_seq,
                );
            }
        }
    }
}
