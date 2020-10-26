use super::icmp;
// use pnet::packet::icmp::IcmpPacket;
use pnet::transport::icmp_packet_iter;
use pnet::transport::transport_channel;
// use pnet::transport::IcmpTransportChannelIterator;
use pnet::transport::{TransportReceiver, TransportSender};
use rand::Rng;
use std::net::IpAddr;
use std::time::Duration;

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

    /// Waits for timeout for icmp packets, then subsequentially
    /// keeps reading until the buffer is exhausted. If an error occurs, will
    /// return the packets read so far, plus the error. Filters those packets that
    /// do not match the ident variable
    pub fn read(&mut self, timeout: Duration) -> (Vec<icmp::PacketData>, Option<std::io::Error>) {
        let mut iter = icmp_packet_iter(&mut self.rx);
        let mut timeout = timeout;
        let mut vec: Vec<icmp::PacketData> = vec![];
        loop {
            match iter.next_with_timeout(timeout) {
                Ok(data) => {
                    if let Some((packet, addr)) = data {
                        let packet = icmp::PacketData::parse(packet, addr);
                        for dest in &self.dest {
                            if packet.ident == dest.ident {
                                vec.push(packet.clone());
                            }
                        }
                        timeout = Duration::from_micros(0);
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
