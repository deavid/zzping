mod icmp;
mod transport;
use std::time::{Duration, Instant};

#[macro_use]
extern crate log;
extern crate env_logger;

fn clearscreen() {
    print!("{esc}[2J{esc}[1;1H", esc = 27 as char);
}
fn main() {
    info!("Program start");
    let interval = Duration::from_millis(5);
    let wait = Duration::from_millis(10);
    let refresh = Duration::from_millis(100);

    let wait_forget_lost = Duration::from_millis(10000);
    let wait_forget_inflight = Duration::from_millis(2000);
    let wait_forget_recv = Duration::from_millis(2000);
    let packet_loss_time1 = Duration::from_millis(200);
    let packet_loss_time2 = Duration::from_millis(500);
    let mut last_refresh = Instant::now() - Duration::from_secs(60);
    let mut t = transport::Comms::new();
    t.add_destination("192.168.0.232", interval);
    t.add_destination("192.168.0.1", interval);
    t.add_destination("8.8.4.4", interval);
    loop {
        t.send_all();
        t.recv_all(wait);

        if last_refresh.elapsed() > refresh {
            for dest in t.dest.iter_mut() {
                for pck in dest
                    .packets
                    .iter()
                    .filter(|x| last_refresh.saturating_duration_since(x.sent) >= wait_forget_recv)
                {
                    dest.lost_packets.push(pck.clone());
                }
                dest.packets.retain(|x| {
                    last_refresh.saturating_duration_since(x.sent) < wait_forget_inflight
                });
                dest.recv_packets
                    .retain(|x| last_refresh.saturating_duration_since(x.sent) < wait_forget_recv);
                dest.lost_packets
                    .retain(|x| last_refresh.saturating_duration_since(x.sent) < wait_forget_lost);
            }
            last_refresh = Instant::now();
            clearscreen();
            for dest in t.dest.iter() {
                let inflight = dest.packets.len();
                let recv_count = dest.recv_packets.len();
                let packets_lost = dest.packets.iter().fold(0, |acc, x| {
                    acc + if last_refresh - x.sent >= packet_loss_time1 {
                        1
                    } else {
                        0
                    }
                }) + dest.lost_packets.len();
                let packets_recv = dest.recv_packets.iter().fold(0, |acc, x| {
                    acc + if last_refresh - (x.sent + x.received.unwrap()) <= packet_loss_time2 {
                        1
                    } else {
                        0
                    }
                });
                let packet_loss =
                    (100.0 * packets_lost as f32) / ((packets_lost + packets_recv) as f32);
                let tot_time: Duration = dest
                    .recv_packets
                    .iter()
                    .fold(Duration::from_micros(0), |acc, x| {
                        acc + x.received.unwrap_or_default()
                    });
                let avg_time: Duration = if dest.recv_packets.is_empty() {
                    Duration::from_millis(999)
                } else {
                    tot_time / (dest.recv_packets.len() as u32)
                };
                let last_pckt_received = dest
                    .recv_packets
                    .last()
                    .map_or(last_refresh, |x| x.sent)
                    .elapsed();
                println!(
                    "{:>14?} - {:>4} in-flight - {:>4.2} recv/s - {:>7.2?}ms / {:>4.1?}s - {:>7.2}% loss ({}/{})",
                    dest.addr,
                    inflight,
                    recv_count as f32 / wait_forget_recv.as_secs_f32(),
                    avg_time.as_secs_f32() * 1000.0,
                    last_pckt_received.as_secs_f32(),
                    packet_loss,
                    packets_lost,
                    packets_recv,
                )
            }
            // for dest in t.dest.iter() {
            //     for p in dest.packets.iter() {
            //         println!(
            //             "{:>12?} - {:>4} - {:#?}",
            //             &p.data.addr,
            //             dest.seq - p.data.seqn, // This overflows
            //             p.received.is_some()
            //         );
            //     }
            // }
        }
    }
}
