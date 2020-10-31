use super::icmp;
// use pnet::packet::icmp::IcmpPacket;
use pnet::transport::icmp_packet_iter;
use pnet::transport::transport_channel;
// use pnet::transport::IcmpTransportChannelIterator;
use pnet::transport::{TransportReceiver, TransportSender};
use rand::Rng;
use std::net::IpAddr;
use std::time::Duration;
use std::time::Instant;

#[derive(Debug)]
pub struct Destination {
    pub addr: IpAddr,
    pub interval: Duration,
    pub seq: u16,   // Next packet number
    pub ident: u16, // Identifier for this queue
    pub inflight_packets: Vec<icmp::PacketSent>,
    pub recv_packets: Vec<icmp::PacketSent>,
    pub lost_packets: Vec<icmp::PacketSent>,
    pub sent_count: u64,
    pub recv_count: u64,
    pub rng: rand::rngs::ThreadRng,
}

impl Destination {
    pub fn new(addr: IpAddr, interval: Duration) -> Self {
        Self {
            addr,
            interval,
            seq: 1,
            ident: rand::thread_rng().gen(),
            inflight_packets: vec![],
            recv_packets: vec![],
            lost_packets: vec![],
            sent_count: 0,
            recv_count: 0,
            rng: rand::thread_rng(),
        }
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
        if self.seq == 65535 {
            // wrap-around
            self.seq = 0;
        } else {
            self.seq += 1;
        }
        self.sent_count += 1;
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
        self.add_dest_addr(icmp::ipaddr(addr).unwrap(), interval);
    }

    pub fn add_dest_addr(&mut self, addr: IpAddr, interval: Duration) {
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
        fn sent_before(pck: &icmp::PacketSent, now: Instant, wait: Duration) -> bool {
            now.saturating_duration_since(pck.sent) < wait
        }
        fn sent_after(pck: &icmp::PacketSent, now: Instant, wait: Duration) -> bool {
            !sent_before(pck, now, wait)
        }
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
