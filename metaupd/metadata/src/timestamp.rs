//! UTC timestamps in RFC 3339 form.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// An RFC 3339 timestamp. Only the `Z` offset is accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Timestamp(OffsetDateTime);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid timestamp: expected RFC 3339 UTC such as 2026-01-31T00:00:00Z")]
pub struct TimestampError;

impl Timestamp {
    /// Current time, truncated to whole seconds.
    pub fn now() -> Self {
        let now = OffsetDateTime::now_utc();
        Self(now.replace_nanosecond(0).unwrap_or(now))
    }

    /// Seconds since 1970; 0 for anything earlier.
    pub fn unix_seconds(self) -> u64 {
        u64::try_from(self.0.unix_timestamp()).unwrap_or(0)
    }

    #[must_use]
    pub fn plus_days(self, days: i64) -> Self {
        Self(self.0.saturating_add(time::Duration::days(days)))
    }

    /// `YYYYMMDDHHMMSS`, for file and boot environment names.
    pub fn compact(self) -> String {
        let t = self.0;
        format!(
            "{:04}{:02}{:02}{:02}{:02}{:02}",
            t.year(),
            u8::from(t.month()),
            t.day(),
            t.hour(),
            t.minute(),
            t.second()
        )
    }
}

impl FromStr for Timestamp {
    type Err = TimestampError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if !text.ends_with('Z') {
            return Err(TimestampError);
        }
        OffsetDateTime::parse(text, &Rfc3339)
            .map(Self)
            .map_err(|_| TimestampError)
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.format(&Rfc3339).map_err(|_| fmt::Error)?)
    }
}

impl TryFrom<String> for Timestamp {
    type Error = TimestampError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<Timestamp> for String {
    fn from(value: Timestamp) -> Self {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_orders() {
        let a: Timestamp = "2026-01-31T00:00:00Z".parse().unwrap();
        assert_eq!(a.to_string(), "2026-01-31T00:00:00Z");
        assert_eq!(a.compact(), "20260131000000");
        assert_eq!(a.plus_days(1).to_string(), "2026-02-01T00:00:00Z");
        assert!(a < a.plus_days(1));
    }

    #[test]
    fn rejects_other_offsets_and_garbage() {
        for text in [
            "2026-01-31T00:00:00+01:00",
            "2026-01-31",
            "Z",
            "",
            "2026-13-01T00:00:00Z",
        ] {
            assert!(text.parse::<Timestamp>().is_err(), "{text:?}");
        }
    }
}
