use chrono::{DateTime, Utc};

pub use std::time::Instant;

pub trait Clock {
    fn utc_now(&self) -> DateTime<Utc>;
    fn now(&self) -> Instant;
}
