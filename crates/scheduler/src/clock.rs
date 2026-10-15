use chrono::{DateTime, Utc};

pub use web_time::Instant;

pub trait Clock {
    fn utc_now(&self) -> DateTime<Utc>;
    fn now(&self) -> Instant;
}
