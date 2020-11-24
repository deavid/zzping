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

use super::icmp;
use pnet::transport::icmp_packet_iter;
use pnet::transport::transport_channel;
use pnet::transport::{TransportReceiver, TransportSender};
use rand::Rng;
use std::io::BufWriter;
use std::net::IpAddr;
use std::time::{Duration, Instant};
use std::{fs::File, io::Write};

fn recv_before(pck: &icmp::PacketSent, now: Instant, wait: Duration) -> bool {
    let received: Duration = pck.received.unwrap_or_default();
    now.saturating_duration_since(pck.sent + received) < wait
}

fn sent_before(pck: &icmp::PacketSent, now: Instant, wait: Duration) -> bool {
    now.saturating_duration_since(pck.sent) < wait
}

fn sent_after(pck: &icmp::PacketSent, now: Instant, wait: Duration) -> bool {
    !sent_before(pck, now, wait)
}

#[derive(Debug)]
pub struct Destination {
    pub addr: IpAddr,
    pub str_addr: String,
    pub interval: Duration,
    pub seq: u16,   // Next packet number
    pub ident: u16, // Identifier for this queue
    pub inflight_packets: Vec<icmp::PacketSent>,
    pub recv_packets: Vec<icmp::PacketSent>,
    pub lost_packets: Vec<icmp::PacketSent>,
    pub sent_count: u64,
    pub recv_count: u64,
    pub rng: rand::rngs::ThreadRng,
    pub logfile: Option<BufWriter<File>>,
}

impl Destination {
    pub fn new(str_addr: &str, interval: Duration) -> Self {
        Self {
            addr: icmp::ipaddr(str_addr).unwrap(),
            str_addr: str_addr.to_owned(),
            interval,
            seq: 1,
            ident: rand::thread_rng().gen(),
            inflight_packets: vec![],
            recv_packets: vec![],
            lost_packets: vec![],
            sent_count: 0,
            recv_count: 0,
            rng: rand::thread_rng(),
            logfile: None,
        }
    }

    pub fn create_log_file(&mut self, now: &str) {
        let filename = format!("pingd-log-{}-{}.log", self.str_addr, now);
        let f = File::create(filename).unwrap();
        let mut oldlog = self.logfile.take();
        if let Some(log) = oldlog.as_mut() {
            log.flush().unwrap();
        }
        // Buffering is needed to avoid wearing SSDs by not writting the same
        // sector dozens of times. 8KB by default. It auto-flushes.
        self.logfile = Some(BufWriter::new(f));
    }

    pub fn recv(&mut self, packet: icmp::PacketData) -> Option<(IpAddr, Duration)> {
        let mut ret: Option<(IpAddr, Duration)> = None;
        for sent in self.inflight_packets.iter_mut() {
            if sent.data.ident == packet.ident && sent.received.is_none() {
                sent.received = Some(sent.sent.elapsed());
                self.recv_count += 1;
                self.recv_packets.push(sent.clone());
                ret = Some((packet.addr, sent.received.unwrap()));
            }
        }
        if ret.is_some() {
            self.inflight_packets.retain(|x| x.received.is_none());
        }
        ret
    }

    pub fn send(&mut self, tx: &mut TransportSender) {
        let inflight = self.inflight_packets.len() as u16;
        /*
         rnd_num and skipping is a hack to avoid a bug creating nasty sizes of
         the queues. It is currently fixed (by looking and cleaning up >1 pckt
         on recv), but the hack stays just in case.
        */
        let rnd_num = self.rng.gen_range(16, 64);
        if rnd_num >= inflight {
            let packet = icmp::PacketData::new(self.seq, self.ident, self.addr).send(tx);
            self.inflight_packets.push(packet);
        } else if rnd_num * 2 >= inflight {
            // TODO: Seems we have a bug and we don't clean the queue properly... ¿dup packets? hmmm
            let idx = self.rng.gen_range(0, inflight) as usize;
            self.inflight_packets.remove(idx);
            return;
        }
        self.seq = self.rng.gen();
        // if self.seq == 65535 {
        //     // wrap-around
        //     self.seq = 0;
        // } else {
        //     self.seq += 1;
        // }
        self.sent_count += 1;
    }

    pub fn received_last(&self, wait: Duration) -> Vec<icmp::PacketSent> {
        let now = Instant::now();
        self.recv_packets
            .iter()
            .filter(|x| recv_before(x, now, wait))
            .cloned()
            .collect()
    }
    pub fn inflight_after(&self, wait: Duration) -> Vec<icmp::PacketSent> {
        let now = Instant::now();
        self.inflight_packets
            .iter()
            .filter(|x| !recv_before(x, now, wait))
            .cloned()
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct CommConfig {
    pub forget_lost: Duration,
    pub forget_inflight: Duration,
    pub forget_recv: Duration,
}

pub struct Comms {
    /// Collection of hosts to send pings to
    pub dest: Vec<Destination>,
    /// Read channel
    rx: TransportReceiver,
    /// Write channel
    tx: TransportSender,
    /// Timings Config
    pub config: CommConfig,
}

impl Comms {
    pub fn new(config: CommConfig) -> Self {
        // let ident: u16 = (std::process::id() % 65536) as u16;
        let buf = 65536;
        // let buf = 1024;
        let (tx, rx) = match transport_channel(buf, icmp::protocol()) {
            Ok((tx, rx)) => (tx, rx),
            Err(e) => panic!(e.to_string()),
        };
        Self {
            dest: vec![],
            rx,
            tx,
            config,
        }
    }

    pub fn add_destination(&mut self, addr: &str, interval: Duration) {
        self.dest.push(Destination::new(addr, interval))
    }

    /// Sends a ping for each destination
    pub fn send_all(&mut self) {
        for dest in &mut self.dest {
            dest.send(&mut self.tx);
        }
    }

    pub fn cleanup(&mut self) {
        let c = self.config.clone();
        let now = Instant::now();
        for dest in self.dest.iter_mut() {
            for pck in dest
                .inflight_packets
                .iter()
                .filter(|x| sent_after(x, now, c.forget_recv))
            {
                dest.lost_packets.push(pck.clone());
            }
            dest.inflight_packets
                .retain(|x| sent_before(x, now, c.forget_inflight));
            dest.recv_packets
                .retain(|x| sent_before(x, now, c.forget_recv));
            dest.lost_packets
                .retain(|x| sent_before(x, now, c.forget_lost));
        }
    }

    pub fn _delay() {
        /*
        The delay could be something evenly spaced. Maybe the formula of:

        1/delay = 1/delay1 + 1/delay2 + ...
        OR
        HZ = Hz1 + Hz2 + Hz3 + ...

        Could get us something evenly spaced. The idea being that if there are
        4 destinations at 40ms, we send one each 10ms, instead of doing all four
        at once every 40ms.
        */
    }

    /// Waits for timeout for icmp packets, then subsequentially
    /// keeps reading until the buffer is exhausted. If an error occurs, will
    /// return the packets read so far, plus the error. Filters those packets that
    /// do not match the ident variable
    pub fn recv_all(
        &mut self,
        timeout: Duration,
    ) -> (Vec<(IpAddr, Duration)>, Option<std::io::Error>) {
        // TODO: Rename to recv_all  & change return to Result::Ok(None)
        let mut iter = icmp_packet_iter(&mut self.rx);
        let mut next_timeout = Duration::from_micros(1000);
        let zero = Duration::from_micros(100);
        let mut vec: Vec<(IpAddr, Duration)> = vec![];
        let starttime = Instant::now();
        loop {
            match iter.next_with_timeout(next_timeout) {
                Ok(data) => {
                    if let Some((packet, addr)) = data {
                        let packet = icmp::PacketData::parse(packet, addr);
                        for dest in &mut self.dest {
                            if packet.ident == dest.ident {
                                if let Some(info) = dest.recv(packet.clone()) {
                                    vec.push(info);
                                }
                            }
                        }
                    } else if next_timeout == zero {
                        break;
                    }
                    if next_timeout != zero && starttime.elapsed() > timeout {
                        next_timeout = zero;
                    }
                }
                Err(e) => {
                    return (vec, Some(e));
                }
            }
        }
        (vec, None)
    }
}
