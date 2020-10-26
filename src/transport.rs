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
    addr: IpAddr,
    interval: Duration,
    seq: u16,   // Next packet number
    ident: u16, // Identifier for this queue
    packets: Vec<icmp::PacketSent>,
}

impl Destination {
    pub fn new(addr: IpAddr, interval: Duration) -> Self {
        Self {
            addr,
            interval,
            seq: 1,
            ident: rand::thread_rng().gen(),
            packets: vec![],
        }
    }

    pub fn recv(&mut self, packet: icmp::PacketData) {
        dbg!(self, packet);
    }
}

pub struct Comms {
    /// Collection of hosts to send pings to
    pub dest: Vec<Destination>,
    /// Read channel
    rx: TransportReceiver,
    /// Write channel
    tx: TransportSender,
}

impl Comms {
    pub fn new() -> Self {
        // let ident: u16 = (std::process::id() % 65536) as u16;
        let (tx, rx) = match transport_channel(65536, icmp::protocol()) {
            Ok((tx, rx)) => (tx, rx),
            Err(e) => panic!(e.to_string()),
        };
        Self {
            dest: vec![],
            rx,
            tx,
        }
    }

    pub fn add_destination(&mut self, addr: IpAddr, interval: Duration) {
        self.dest.push(Destination::new(addr, interval))
    }

    /// Sends a ping for each destination
    pub fn send_all(&mut self) {
        for dest in &mut self.dest {
            let packet = icmp::PacketData::new(dest.seq, dest.ident, dest.addr).send(&mut self.tx);
            dest.seq += 1; // WARN: Should wrap-around
            dest.packets.push(packet);
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
    pub fn read(&mut self, timeout: Duration) -> (Vec<icmp::PacketData>, Option<std::io::Error>) {
        // TODO: Rename to recv_all  & change return to Result::Ok(None)
        let mut iter = icmp_packet_iter(&mut self.rx);
        let mut next_timeout = timeout;
        let mut vec: Vec<icmp::PacketData> = vec![];
        let starttime = Instant::now();
        loop {
            match iter.next_with_timeout(next_timeout) {
                Ok(data) => {
                    if let Some((packet, addr)) = data {
                        let packet = icmp::PacketData::parse(packet, addr);
                        for dest in &mut self.dest {
                            if packet.ident == dest.ident {
                                vec.push(packet.clone()); // DELETE THIS
                                dest.recv(packet.clone());
                            }
                        }
                        let elapsed = starttime.elapsed();
                        if elapsed + Duration::from_micros(1) > timeout {
                            break;
                        }
                        // TODO: If all packets are already consumed, return early!
                        next_timeout = timeout - elapsed;
                    } else {
                        break;
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
