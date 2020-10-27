mod icmp;
mod transport;
use std::time::Duration;

#[macro_use]
extern crate log;
extern crate env_logger;

fn main() {
    info!("Program start");
    let interval = Duration::from_millis(5);
    let wait = Duration::from_millis(100);

    let mut t = transport::Comms::new();
    t.add_destination("192.168.0.1", interval);
    t.add_destination("8.8.4.4", interval);
    t.add_destination("127.0.0.1", interval);
    loop {
        t.send_all();
        let (vpck, err) = t.recv_all(wait);
        if let Some(err) = err {
            dbg!(err);
        }
        for recv in vpck {
            println!("{:?}\t{:?}", recv.0, recv.1);
        }
    }
}
