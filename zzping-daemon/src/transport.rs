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

//! ICMP Transport and management objects
//!
//! This is the core of the zzping-daemon binary, it holds its main behavior.
//!

use super::icmp;
use pnet::transport::{TransportChannelType, TransportReceiver, TransportSender};
use rand::Rng;
use std::{fs::File, io::Write};
use std::{io::BufWriter, sync::Mutex};
use std::{net::IpAddr, sync::Arc};
use std::{
    thread,
    time::{Duration, Instant},
};

/// Creates a TransportChannelType for ICMP over IPv4
pub fn protocol_ipv4() -> TransportChannelType {
    use pnet::packet::ip::IpNextHeaderProtocols;
    use pnet::transport::TransportChannelType::Layer4;
    use pnet::transport::TransportProtocol::Ipv4;
    Layer4(Ipv4(IpNextHeaderProtocols::Icmp))
}

/// Parses a string into an IP Address.
pub fn parse_ipaddr(ipaddr: &str) -> Option<IpAddr> {
    // TODO: This function is basically useless. What do we do with it?
    let addr = ipaddr.parse::<IpAddr>();
    match addr {
        Ok(valid_addr) => Some(valid_addr),
        Err(e) => {
            error!("Error parsing ip address {}. Error: {}", ipaddr, e);
            None
        }
    }
}

/// Function used for .filter() so it can parse a queue and get packets that were
/// received before "now" - "wait".
pub fn recv_before(pck: &icmp::PacketSent, now: Instant, wait: Duration) -> bool {
    let received: Duration = pck.received.unwrap_or_default();
    now.saturating_duration_since(pck.sent + received) < wait
}

/// Function used for .filter() so it can parse a queue and get packets that were
/// sent before "now" - "wait".
pub fn sent_before(pck: &icmp::PacketSent, now: Instant, wait: Duration) -> bool {
    now.saturating_duration_since(pck.sent) < wait
}

/// Function used for .filter() so it can parse a queue and get packets that were
/// sent after "now" - "wait".
pub fn sent_after(pck: &icmp::PacketSent, now: Instant, wait: Duration) -> bool {
    !sent_before(pck, now, wait)
}

/// Defines a destination host with parameters and internal queues.
#[derive(Debug)]
pub struct Destination {
    /// String Address, as used when creating this destination.
    pub str_addr: String,

    /// Limit on how frequently to send pings to the target host.
    pub interval: Duration,

    /// Target Host address.
    pub addr: IpAddr,

    /// Next ICMP packet number to be sent.
    pub seq: u16,

    /// ICMP Identifier for this queue
    pub ident: u16,

    /// When was the last packet sent
    pub last_pckt_sent: Instant,

    /// Queue of packets sent awaiting for response.
    ///
    /// When received, they move to recv_packets. If a certain amount of time
    /// has passed, they're deemed lost and moved to lost_packets.
    pub inflight_packets: Vec<icmp::PacketSent>,

    /// Queue of packets received.
    ///
    /// This queue is hold only for a certain amount of time. After that, the
    /// packets are removed.
    pub recv_packets: Vec<icmp::PacketSent>,

    /// Queue of lost packets.
    ///
    /// This queue is hold only for a certain amount of time. After that, the
    /// packets are removed.
    pub lost_packets: Vec<icmp::PacketSent>,

    /// Stat counter of amount of packets sent to this destination.
    ///
    /// For stats only, this will be reset each time the program restarts.
    pub sent_count: u64,

    /// Stat counter of amount of packets received from this destination.
    ///
    /// For stats only, this will be reset each time the program restarts.
    pub recv_count: u64,

    /// Thread Random generator. Used only for caching purposes.
    pub rng: rand::rngs::ThreadRng,

    /// Where to write the packets to disk. To be deprecated.
    pub logfile: Option<BufWriter<File>>,
}

impl Destination {
    /// Create a new destination from a IP Address in a string and a interval
    /// for the frequency of the pings.
    pub fn new(str_addr: &str, interval: Duration) -> Self {
        Self {
            addr: parse_ipaddr(str_addr).unwrap(),
            str_addr: str_addr.to_owned(),
            last_pckt_sent: Instant::now() - interval,
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

    /// Enables logging packets to disk. If there was a logging running, it will
    /// switch to the new file. If the file exists, it will be replaced by a new
    /// one.
    ///
    /// The filename follows the format ./logs/pingd-log-{str_addr}-{now}.log
    pub fn create_log_file(&mut self, now: &str) {
        let filename = format!("logs/pingd-log-{}-{}.log", self.str_addr, now);
        let f = File::create(filename).unwrap();
        let mut oldlog = self.logfile.take();
        if let Some(log) = oldlog.as_mut() {
            log.flush().unwrap();
        }
        // Buffering is needed to avoid wearing SSDs by not writting the same
        // sector dozens of times. 8KB by default. It auto-flushes.
        self.logfile = Some(BufWriter::new(f));
    }

    /// Try to match an incoming packet against the inflight_packets queue.
    ///
    /// If the packet is one that we sent, this function will complete the
    /// packet with the elapsed time of the response and move it to the
    /// recv_packets queue, removing it from the inflight_packets queue.
    pub fn recv(&mut self, packet: &icmp::PacketData) -> Option<(IpAddr, Duration)> {
        // TODO: A queue to detect duplicate responses would be nice to have.
        // TODO: Part of this code belongs to icmp::PacketSent::recv.
        // TODO: This code should consume PacketSent and craft a PacketReceived.
        let mut ret: Option<(IpAddr, Duration)> = None;
        if self.ident != packet.ident {
            return ret;
        }
        for sent in self.inflight_packets.iter_mut() {
            if sent.data.seqn == packet.seqn && sent.received.is_none() {
                sent.received = match packet.received {
                    Some(received) => received.checked_duration_since(sent.sent),
                    None => Some(sent.sent.elapsed()),
                };
                if sent.received.is_none() {
                    // Received before sending. This must be because a duplicate packet was matched.
                    // TODO: fix duplicate in-flight packets
                    continue;
                }
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

    /// Send another ping to this destination
    ///
    /// If the destination keeps creeping up in the inflight_packets (not responding)
    /// then this function will randomly be a no-op to avoid DoS to a device, and
    /// also to avoid having insane amounts of packets to search later.
    pub fn send(&mut self, tx: &mut TransportSender, min_delay: Duration) -> bool {
        let inflight = self.inflight_packets.len() as u16;
        /*
         rnd_num and skipping is a hack to avoid a bug creating nasty sizes of
         the queues. It is currently fixed (by looking and cleaning up >1 pckt
         on recv), but the hack stays just in case.
        */
        let rnd_num = self.rng.gen_range(16, 64);
        if rnd_num < inflight {
            return false;
        }
        if self.last_pckt_sent.elapsed() + min_delay < self.interval {
            return false;
        }
        let packet = icmp::PacketData::new(self.seq, self.ident, self.addr).send(tx);
        self.last_pckt_sent = Instant::now() - Duration::from_micros(self.rng.gen_range(0, 101));
        self.inflight_packets.push(packet);

        // The sequence is random to avoid a device "guessing" what the next sequence will be.
        // TODO: This opens the door to sending two packets with the same seq number.
        self.seq = self.rng.gen();
        self.sent_count += 1;
        true
    }

    /// Return the packets that were received on the last "wait" seconds.
    ///
    /// This clones the packets, so it might be a bit intensive.
    pub fn received_last(&self, wait: Duration) -> Vec<icmp::PacketSent> {
        let now = Instant::now();
        self.recv_packets
            .iter()
            .filter(|x| recv_before(x, now, wait))
            .cloned()
            .collect()
    }

    /// Return the packets that are awaiting for response and sent in the last "wait" seconds.
    ///
    /// This clones the packets, so it might be a bit intensive.
    pub fn inflight_after(&self, wait: Duration) -> Vec<icmp::PacketSent> {
        let now = Instant::now();
        self.inflight_packets
            .iter()
            .filter(|x| !recv_before(x, now, wait))
            .cloned()
            .collect()
    }

    /// Calculate the average time that packets are taking to return over a period of time.
    pub fn mean_recv_time(&self, time_avg: Duration) -> Option<Duration> {
        if self.recv_packets.is_empty() {
            return None;
        }
        let avg: Vec<_> = self
            .recv_packets
            .iter()
            .filter(|x| (x.sent + x.received.unwrap()).elapsed() < time_avg)
            .collect();
        let avg_len = avg.len().max(1);
        let tot_time: Duration = avg.iter().fold(Duration::from_micros(0), |acc, x| {
            acc + x.received.unwrap_or_default()
        });
        let avg_time: Duration = tot_time / (avg_len as u32);

        Some(avg_time)
    }
}

/// Configuration struct used to create new Comms objects
#[derive(Debug, Clone, Copy)]
pub struct CommConfig {
    /// Time required to consider an inflight packet lost.
    pub forget_inflight: Duration,
    /// How long lost packets are hold.
    pub forget_lost: Duration,
    /// How long received packets are hold.
    pub forget_recv: Duration,
    /// Timing precision multiplier, makes the wait smaller
    pub precision_mult: f64,
    // TODO: Add TransportChannelType here?, so it can configure IpV4 or IpV6.
}

/// Pinger struct to manage send/recv pings for several destinations at different intervals.
pub struct Comms {
    /// Collection of hosts to send pings to
    pub dest: Vec<Destination>,
    /// Write channel
    tx: TransportSender,
    /// Timings Config
    pub config: CommConfig,
    /// Recommended delay
    pub delay: Duration,
    // ---- Reader Thread Data ----
    /// Buffer for the reader thread to fill, will be emptied in recv_all
    readbuf: Arc<Mutex<Vec<icmp::PacketData>>>,
    /// Handle of the thread for joining. Unused, as the thread never ends
    _read_thread_handle: thread::JoinHandle<()>,
}

/// Reader thread implementation
///
/// This continuosly reads from the socket and sends the data to the main thread
/// every 0.1ms. This is done to prevent blocking, also mutexes are expensive.
fn receiver_thread(mut rx: TransportReceiver, readbuf: Arc<Mutex<Vec<icmp::PacketData>>>) {
    let mut buffer: Vec<icmp::PacketData> = vec![];
    let mut last_sync = Instant::now();
    let sync_time = Duration::from_micros(100);
    let mut packet_iter = pnet::transport::icmp_packet_iter(&mut rx);
    loop {
        if let Some((packet, addr)) = packet_iter.next_with_timeout(sync_time).unwrap_or_default() {
            let mut packet: icmp::PacketData = icmp::PacketData::parse(packet, addr);
            packet.received = Some(Instant::now());
            buffer.push(packet);
        }

        if last_sync.elapsed() > sync_time {
            // Try to lock the buffer, if it would block, just try later. Don't block!!
            if let Ok(mut locked_buffer) = readbuf.try_lock() {
                // Dump our state to the external buffer
                locked_buffer.append(&mut buffer);
                last_sync = Instant::now();
            }
        }
    }
}

impl std::fmt::Debug for Comms {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Comms")
            .field("dest", &self.dest)
            .field("config", &self.config)
            .finish()
    }
}

impl Comms {
    /// Create a new Comms object from config
    pub fn new(config: CommConfig) -> Self {
        let bufsize = 65536;
        // TODO: Caller should have two Comms, one for IpV4, and another for IpV6.
        let (tx, rx) = match pnet::transport::transport_channel(bufsize, protocol_ipv4()) {
            Ok((tx, rx)) => (tx, rx),
            Err(e) => panic!("{}", e.to_string()),
        };
        // rx is sent to the thread as an exclusive thing, we lose track of it here.
        let readbuf = Arc::new(Mutex::new(vec![]));
        let thread_buf = readbuf.clone();
        let read_thread_handle: thread::JoinHandle<()> =
            std::thread::spawn(move || receiver_thread(rx, thread_buf));
        Self {
            dest: vec![],
            tx,
            config,
            delay: Duration::from_millis(1),
            readbuf,
            _read_thread_handle: read_thread_handle,
        }
    }
    /// Add a new destination from a given string address
    pub fn add_destination(&mut self, addr: &str, interval: Duration) {
        if interval.as_nanos() == 0 {
            panic!("Interval for a target host cannot be zero.")
        }
        self.dest.push(Destination::new(addr, interval));
        self.delay = self.get_delay();
    }

    /// Sends a ping for each destination
    pub fn send_all(&mut self, limit: usize) -> usize {
        let mut count = 0;
        let mut dests: Vec<_> = self
            .dest
            .iter()
            .map(|x| {
                x.last_pckt_sent
                    .saturating_duration_since(Instant::now())
                    .as_micros() as i128
            })
            .enumerate()
            .collect();
        dests.sort_unstable_by_key(|(_, x)| -*x);
        let delay = self.delay / 2;
        for (n, _) in dests {
            if self.dest[n].send(&mut self.tx, delay) {
                count += 1;
                if limit > 0 && count >= limit {
                    break;
                }
            }
        }
        count
    }

    /// Forget old packets following the config specs.
    pub fn cleanup(&mut self) {
        let c = self.config;
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
                .retain(|x| sent_before(x, now, c.forget_inflight + c.forget_recv));
            dest.lost_packets
                .retain(|x| sent_before(x, now, c.forget_inflight + c.forget_lost));
        }
    }

    /// Estimate the desired delay to complete all packets in one second
    pub fn get_delay(&self) -> Duration {
        let mut freq = 0.0;
        for dest in &self.dest {
            freq += dest.interval.as_secs_f64().recip();
        }
        // Extra precision, will check X times faster
        freq *= self.config.precision_mult;
        dbg!(freq);
        if freq > 0.0 {
            Duration::from_secs_f64(freq.recip())
        } else {
            // Sensible default when there are no targets.
            // It might also be that all targets have interval=0. Wrong, but whatever.
            Duration::from_millis(1)
        }
    }

    /// Listens for packets up to "timeout" time and matches incoming packets
    /// with the different destinations and their recv queues.
    ///
    /// There is a thread reading the socket continuosly, this function just
    /// dumps that data into local memory and matches the packets against the
    /// different target hosts.
    pub fn recv_all(&mut self, timeout: Duration) {
        let starttime = Instant::now();
        if timeout.as_millis() > 5000 {
            dbg!(timeout);
            panic!("recv_all: Tried to wait more than 5000ms");
        }
        loop {
            let mut buffer = vec![];
            let mut locked_buffer = self.readbuf.lock().unwrap();
            buffer.append(&mut locked_buffer);
            // Release the lock early once the Vec is empty.
            drop(locked_buffer);
            // Now we can work freely with the values.
            for packet in buffer {
                for dest in &mut self.dest {
                    dest.recv(&packet);
                }
            }
            if let Some(wait) = timeout.checked_sub(starttime.elapsed()) {
                assert!(wait <= timeout);
                thread::sleep(wait);
            } else {
                break;
            }
        }
    }
}

// TODO: Add tests here:
#[cfg(test)]
mod tests {
    /*
    use super::*;

    #[test]
    fn test_XXX() {}
    */
}
