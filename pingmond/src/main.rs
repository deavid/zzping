mod icmp;
mod transport;
// use std::io;
use std::net::UdpSocket;
use std::time::{Duration, Instant};

#[macro_use]
extern crate log;
extern crate env_logger;
extern crate rmp;

fn clearscreen() {
    print!("{esc}[2J{esc}[1;1H", esc = 27 as char);
}
fn main() {
    info!("Program start");
    let socket = UdpSocket::bind("127.0.0.1:7878").unwrap();
    socket.set_nonblocking(true).unwrap();
    let udp_dest = "127.0.0.1:7879";
    // socket
    //     .connect("127.0.0.1:7879")
    //     .expect("connect function failed");
    // socket.send(&[0, 1, 2]).expect("couldn't send message");

    let interval = Duration::from_millis(5);
    let wait = Duration::from_millis(10);
    let refresh = Duration::from_millis(200);

    let pckt_loss_inflight_time = Duration::from_millis(100);
    let pckt_loss_recv_time = Duration::from_millis(300);
    let time_avg = Duration::from_millis(200);
    let mut last_refresh = Instant::now() - Duration::from_secs(60);
    let mut t = transport::Comms::new(transport::CommConfig {
        forget_lost: Duration::from_millis(10000),
        forget_inflight: Duration::from_millis(2000),
        forget_recv: Duration::from_millis(2000),
    });
    t.add_destination("192.168.0.232", interval);
    t.add_destination("192.168.0.1", interval);
    t.add_destination("8.8.4.4", interval);
    loop {
        t.send_all();
        t.recv_all(wait);

        if last_refresh.elapsed() > refresh {
            last_refresh = Instant::now();
            t.cleanup();
            clearscreen();
            let mut udp_ok = true;
            for dest in t.dest.iter() {
                let inflight_count = dest.inflight_packets.len();
                let recv_count = dest.recv_packets.len();
                let packets_lost = dest.inflight_packets.iter().fold(0, |acc, x| {
                    acc + if last_refresh - x.sent >= pckt_loss_inflight_time {
                        1
                    } else {
                        0
                    }
                }) + dest.lost_packets.len();
                let packets_recv = dest.recv_packets.iter().fold(0, |acc, x| {
                    acc + if last_refresh - (x.sent + x.received.unwrap()) <= pckt_loss_recv_time {
                        1
                    } else {
                        0
                    }
                });
                let packet_loss =
                    (100.0 * packets_lost as f32) / ((packets_lost + packets_recv) as f32);
                let avg = dest
                    .recv_packets
                    .iter()
                    .filter(|x| (x.sent + x.received.unwrap()).elapsed() < time_avg);
                let tot_time: Duration = avg.clone().fold(Duration::from_micros(0), |acc, x| {
                    acc + x.received.unwrap_or_default()
                });
                let avg_len = avg.count().max(1);
                let avg_time: Duration = if dest.recv_packets.is_empty() {
                    Duration::from_millis(999)
                } else {
                    tot_time / (avg_len as u32)
                };
                let last_pckt_received = dest
                    .recv_packets
                    .last()
                    .map_or(last_refresh, |x| x.sent)
                    .elapsed();
                let recv_per_sec = recv_count as f32 / t.config.forget_recv.as_secs_f32();
                println!(
                    "{:>14?} - {:>4} in-flight - {:>4.2} recv/s - {:>7.2?}ms / {:>4.1?}s - {:>7.2}% loss ({}/{})",
                    dest.addr,
                    inflight_count,
                    recv_per_sec,
                    avg_time.as_secs_f32() * 1000.0,
                    last_pckt_received.as_secs_f32(),
                    packet_loss,
                    packets_lost,
                    packets_recv,
                );
                let msg = encode_stats(
                    dest.addr,
                    inflight_count,
                    avg_time.as_micros(),
                    last_pckt_received.as_millis(),
                    (packet_loss * 1000.0) as u32,
                );
                udp_ok = udp_ok && socket.send_to(&msg, udp_dest).is_ok();
            }
            if !udp_ok {
                println!("Error sending via UDP. Client might not be connected.")
            }
        }
    }
}

fn encode_stats(
    addr: std::net::IpAddr,
    inflight_count: usize,
    avg_time_us: u128,
    last_pckt_ms: u128,
    packet_loss_x100_000: u32,
) -> Vec<u8> {
    let mut v: Vec<u8> = vec![];
    let addr = addr.to_string();
    rmp::encode::write_array_len(&mut v, 5).unwrap();
    rmp::encode::write_str(&mut v, &addr).unwrap();
    rmp::encode::write_u16(&mut v, inflight_count as u16).unwrap();
    rmp::encode::write_u32(&mut v, avg_time_us as u32).unwrap();
    rmp::encode::write_u32(&mut v, last_pckt_ms as u32).unwrap();
    rmp::encode::write_u32(&mut v, packet_loss_x100_000).unwrap();
    v
}
