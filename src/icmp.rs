use pnet::packet::icmp::echo_reply::MutableEchoReplyPacket;
use pnet::packet::icmp::IcmpTypes;
use pnet::packet::icmp::{echo_request, IcmpPacket};
use pnet::packet::Packet;
use pnet::transport::TransportChannelType;
use pnet::transport::TransportSender;
use pnet::util;
use pnet_macros_support::types::u16be;
// use rand::random;
use std::net::IpAddr;
use std::time::{Duration, Instant, SystemTime};

#[derive(Debug, Clone)]
pub struct PacketData {
    pub seqn: u16,
    pub ident: u16,
    pub addr: IpAddr,
}

impl PacketData {
    pub fn new(seqn: u16, ident: u16, addr: IpAddr) -> Self {
        Self { seqn, ident, addr }
    }
    pub fn parse(packet: IcmpPacket, addr: IpAddr) -> Self {
        let mut pck = packet.packet().to_vec();
        let packet = MutableEchoReplyPacket::new(&mut pck).unwrap();
        Self {
            seqn: packet.get_sequence_number(),
            ident: packet.get_identifier(),
            addr,
        }
    }
    pub fn send(self, tx: &mut TransportSender) -> PacketSent {
        PacketSent::new(self, tx)
    }
}

#[derive(Debug)]
pub struct PacketSent {
    data: PacketData,
    result: std::io::Result<usize>,
    sent: Instant,
    when: SystemTime,
    received: Option<Duration>,
}

impl PacketSent {
    pub fn new(data: PacketData, tx: &mut TransportSender) -> Self {
        let mut payload = vec![0; 16];
        let mut echo_packet =
            echo_request::MutableEchoRequestPacket::new(&mut payload[..]).unwrap();
        echo_packet.set_sequence_number(data.seqn);
        echo_packet.set_identifier(data.ident);
        echo_packet.set_icmp_type(IcmpTypes::EchoRequest);

        let csum = icmp_checksum(&echo_packet);
        echo_packet.set_checksum(csum);

        let result = tx.send_to(echo_packet, data.addr);
        Self {
            data,
            result,
            sent: Instant::now(),
            when: SystemTime::now(),
            received: None,
        }
    }
}

fn icmp_checksum(packet: &echo_request::MutableEchoRequestPacket) -> u16be {
    util::checksum(packet.packet(), 1)
}

pub fn ipaddr(ipaddr: &str) -> Option<IpAddr> {
    let addr = ipaddr.parse::<IpAddr>();
    match addr {
        Ok(valid_addr) => Some(valid_addr),
        Err(e) => {
            error!("Error parsing ip address {}. Error: {}", ipaddr, e);
            None
        }
    }
}

pub fn protocol() -> TransportChannelType {
    use pnet::packet::ip::IpNextHeaderProtocols;
    use pnet::transport::TransportChannelType::Layer4;
    use pnet::transport::TransportProtocol::Ipv4;
    Layer4(Ipv4(IpNextHeaderProtocols::Icmp))
}
