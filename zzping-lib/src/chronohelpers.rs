use chrono::{Utc, TimeZone};

pub trait ChronoHelperDuration {
    fn as_secs_f64(&self) -> f64;
}

impl ChronoHelperDuration for chrono::Duration {
    fn as_secs_f64(&self) -> f64 {
        match self.num_microseconds() {
            Some(us) => us as f64 / 1_000_000.0,
            None => self.num_milliseconds() as f64 / 1_000.0,
        }
    }
}

pub trait ChronoHelperDatetime {
    fn now() -> Self;
    fn unix_epoch() -> Self;
}

impl ChronoHelperDatetime for chrono::DateTime<Utc> {
    fn now() -> Self {
        Utc::now()
    }

    fn unix_epoch() -> Self {
        Utc.timestamp(0, 0)
    }
}
