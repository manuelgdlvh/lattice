//! Timestamp primitives and a `Clock` abstraction.
//!
//! Using our own newtype around `time::OffsetDateTime` lets us:
//! - enforce RFC-3339 UTC serialization everywhere,
//! - make tests deterministic via a `FixedClock`,
//! - keep the public API narrow.

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// Timestamp in UTC, serialized as an RFC-3339 string.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(pub OffsetDateTime);

impl Timestamp {
    pub fn now() -> Self {
        Self(OffsetDateTime::now_utc())
    }

    /// Parse an RFC-3339 string.
    pub fn parse(s: &str) -> Result<Self, time::error::Parse> {
        OffsetDateTime::parse(s, &Rfc3339).map(Self)
    }

    /// RFC-3339 UTC rendering. Always succeeds for a valid `OffsetDateTime`.
    pub fn to_rfc3339(self) -> String {
        self.0
            .format(&Rfc3339)
            .unwrap_or_else(|_| String::from("1970-01-01T00:00:00Z"))
    }
}

impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_rfc3339())
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

/// Injectable clock for deterministic testing.
pub trait Clock: Send + Sync + std::fmt::Debug {
    fn now(&self) -> Timestamp;
}

/// Default production clock: wall-clock UTC.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}

/// Test clock you can pin to a fixed instant and/or advance explicitly.
#[derive(Clone, Debug)]
pub struct FixedClock {
    now: Arc<Mutex<Timestamp>>,
}

impl FixedClock {
    pub fn at(t: Timestamp) -> Self {
        Self {
            now: Arc::new(Mutex::new(t)),
        }
    }

    pub fn set(&self, t: Timestamp) {
        if let Ok(mut guard) = self.now.lock() {
            *guard = t;
        }
    }
}

impl Clock for FixedClock {
    fn now(&self) -> Timestamp {
        self.now.lock().map_or_else(|_| Timestamp::now(), |g| *g)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_roundtrips_via_rfc3339() {
        let t = Timestamp::parse("2026-04-24T10:12:00Z").unwrap();
        assert_eq!(t.to_rfc3339(), "2026-04-24T10:12:00Z");
    }

    #[test]
    fn timestamp_serde_is_string() {
        let t = Timestamp::parse("2026-04-24T10:12:00Z").unwrap();
        let j = serde_json::to_string(&t).unwrap();
        assert_eq!(j, "\"2026-04-24T10:12:00Z\"");
        let back: Timestamp = serde_json::from_str(&j).unwrap();
        assert_eq!(t, back);
    }

    #[test]
    fn fixed_clock_returns_set_value() {
        let t0 = Timestamp::parse("2026-01-01T00:00:00Z").unwrap();
        let clock = FixedClock::at(t0);
        assert_eq!(clock.now(), t0);
        let t1 = Timestamp::parse("2026-06-01T00:00:00Z").unwrap();
        clock.set(t1);
        assert_eq!(clock.now(), t1);
    }
}
