mod icmp;
mod transport;

#[macro_use]
extern crate log;
extern crate env_logger;

fn main() {
    use pnet::transport::icmp_packet_iter;
    use pnet::transport::transport_channel;
    info!("Program start");
    let addr = icmp::ipaddr("192.168.0.1").unwrap();
    let (mut tx, mut rx) = match transport_channel(65536, icmp::protocol()) {
        Ok((tx, rx)) => (tx, rx),
        Err(e) => panic!(e.to_string()),
    };
    let pid: u16 = (std::process::id() % 65536) as u16;
    let seqn = 1;
    let p = icmp::PacketData::new(seqn, pid, addr).send(&mut tx);
    dbg!(p);

    let mut iter = icmp_packet_iter(&mut rx);
    loop {
        match iter.next() {
            Ok((packet, addr)) => {
                let packet = icmp::PacketData::parse(packet, addr);
                if packet.ident == pid {
                    dbg!(packet);
                }
            }
            Err(e) => {
                error!("An error occurred while reading: {}", e);
            }
        }
    }
}
