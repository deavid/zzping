use chrono::{DateTime, Utc};
use std::time::Duration;

pub enum FrameTime {
    Timestamp(DateTime<Utc>),
    Elapsed(Duration),
}
pub struct FrameData {
    pub time: FrameTime,
    pub inflight: usize,
    pub lost_packets: usize,
    pub recv_us: Vec<u128>,
}

impl FrameData {
    pub fn encode<W: std::io::Write>(
        &self,
        wr: &mut W,
    ) -> Result<(), rmp::encode::ValueWriteError> {
        let mut v: Vec<u8> = vec![];
        match self.time {
            FrameTime::Timestamp(now) => {
                let strnow = now.to_rfc3339_opts(chrono::SecondsFormat::Micros, false);
                rmp::encode::write_str(&mut v, &strnow)?;
                rmp::encode::write_u32(&mut v, 0)?;
            }
            FrameTime::Elapsed(elapsed) => {
                rmp::encode::write_u32(&mut v, elapsed.as_micros() as u32)?;
            }
        }
        rmp::encode::write_u16(&mut v, self.inflight as u16)?;
        rmp::encode::write_u16(&mut v, self.lost_packets as u16)?;

        rmp::encode::write_array_len(&mut v, self.recv_us.len() as u32)?;
        for val in self.recv_us.iter() {
            rmp::encode::write_u32(&mut v, *val as u32)?;
        }
        wr.write(&v)
            .map_err(rmp::encode::ValueWriteError::InvalidDataWrite)?;
        Ok(())
    }
}
