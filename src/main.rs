mod icmp;
mod transport;
use std::time::Duration;

#[macro_use]
extern crate log;
extern crate env_logger;

fn main() {
    info!("Program start");
    let addr = icmp::ipaddr("192.168.0.1").unwrap();
    let addr2 = icmp::ipaddr("8.8.4.4").unwrap();
    let interval = Duration::from_millis(5);

    let mut t = transport::Comms::new();
    t.add_destination(addr, interval);
    t.add_destination(addr2, interval);
    t.send_all();
    t.send_all();
    std::thread::sleep(Duration::from_millis(100));
    let (vpck, err) = t.read(interval);
    dbg!(vpck);
    dbg!(err);
}
