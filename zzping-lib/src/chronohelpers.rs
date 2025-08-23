use chrono::{TimeZone, Utc};

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
    fn timestamp_f64(&self) -> f64;
}

impl ChronoHelperDatetime for chrono::DateTime<Utc> {
    fn now() -> Self {
        Utc::now()
    }

    fn unix_epoch() -> Self {
        Utc.timestamp_opt(0, 0).single().expect("Invalid timestamp")
    }

    fn timestamp_f64(&self) -> f64 {
        // WARN: These will have less than microsecond precision past year 2255
        let unix_epoch = Utc.timestamp_opt(0, 0).single().expect("Invalid timestamp");
        self.signed_duration_since(unix_epoch).as_secs_f64()
    }
}
