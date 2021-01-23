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

//! ICMP Packet structs
//!
//! This module contains the required structs to hold details on ICMP packets
//! to be sent or received.
//!

use pnet::packet::icmp::echo_reply::MutableEchoReplyPacket;
use pnet::packet::icmp::IcmpTypes;
use pnet::packet::icmp::{echo_request, IcmpPacket};
use pnet::packet::Packet;
use pnet::transport::TransportSender;
use pnet::util;

use std::net::IpAddr;
use std::time::{Duration, Instant, SystemTime};

/// Describes an ICMP Packet; Usually not sent yet, unless inside of PacketSent.
#[derive(Debug, Clone)]
pub struct PacketData {
    /// ICMP Sequence number to be used when sending.
    pub seqn: u16,
    /// ICMP Identifier to be used when sending.
    pub ident: u16,
    /// Address to send this packet to.
    pub addr: IpAddr,
    /// Time when it was received (if it was). Used to compute later the timing
    pub received: Option<Instant>,
}

impl PacketData {
    /// Construct a new ICMP packet (to be sent later).
    pub fn new(seqn: u16, ident: u16, addr: IpAddr) -> Self {
        Self {
            seqn,
            ident,
            addr,
            received: None,
        }
    }
    /// Parse a received ICMP Packet from given address.
    pub fn parse(packet: IcmpPacket, addr: IpAddr) -> Self {
        let mut pck = packet.packet().to_vec();
        let packet = MutableEchoReplyPacket::new(&mut pck).unwrap();
        Self {
            seqn: packet.get_sequence_number(),
            ident: packet.get_identifier(),
            addr,
            received: None,
        }
    }
    /// Send this ICMP packet using the given transport sender.
    pub fn send(self, tx: &mut TransportSender) -> PacketSent {
        PacketSent::new(self, tx)
    }
    /// Constructs an EchoRequestPacket so it can be sent via TransportSender.
    pub fn create_echo_packet<'a>(
        &self,
        payload: &'a mut [u8],
    ) -> echo_request::MutableEchoRequestPacket<'a> {
        let mut echo_packet = echo_request::MutableEchoRequestPacket::new(payload).unwrap();
        echo_packet.set_sequence_number(self.seqn);
        echo_packet.set_identifier(self.ident);
        echo_packet.set_icmp_type(IcmpTypes::EchoRequest);

        let skip_words = 1;
        let csum = util::checksum(echo_packet.packet(), skip_words);
        echo_packet.set_checksum(csum);
        echo_packet
    }
}

/// Describes an ICMP packet that was sent and possibly awaiting for response.
#[derive(Debug, Clone)]
pub struct PacketSent {
    /// Packet that was sent, dst adress, sequence and ident are here.
    pub data: PacketData,
    /// When it was sent, monotonic clock. (Used to calculate received Duration later)
    pub sent: Instant,
    /// When it was sent, system clock.
    pub when: SystemTime,
    /// Wether it was received, and how long it took to be received.
    pub received: Option<Duration>,
}

// TODO: Create a PacketReceived:  (And remove Option<Duration>)
/*
pub struct PacketReceived {
    pub packet: PacketSent,
    pub received: Duration,
}
 */

impl PacketSent {
    /// Send a PacketData using the TransportSender specified. Constructs a PacketSent with the details.
    pub fn new(data: PacketData, tx: &mut TransportSender) -> Self {
        let mut payload = vec![0; 16];
        let echo_packet = data.create_echo_packet(&mut payload[..]);

        tx.send_to(echo_packet, data.addr).unwrap();
        Self {
            data,
            sent: Instant::now(),
            when: SystemTime::now(),
            received: None,
        }
    }
    // TODO: This lacks a receiving method. Code probably exists in transport.rs.
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
