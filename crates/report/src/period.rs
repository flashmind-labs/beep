use chrono::{DateTime, Local, Timelike};
use serde::{Deserialize, Serialize};
use strum::{EnumString, IntoStaticStr};

/// How far back a report reaches.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, EnumString, IntoStaticStr,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum Period {
    Hour,
    #[default]
    Today,
    Day,
}

impl Period {
    /// `since` from the current local time.
    pub fn since_now(self) -> i64 {
        self.since(Local::now())
    }

    /// Start of the period as Unix seconds: now minus 1 h, local midnight, or now minus 24 h.
    pub fn since(self, now: DateTime<Local>) -> i64 {
        match self {
            Period::Hour => now.timestamp() - 3600,
            Period::Day => now.timestamp() - 86400,
            Period::Today => now
                .date_naive()
                .and_hms_opt(0, 0, 0)
                .and_then(|midnight| midnight.and_local_timezone(Local).earliest())
                .map(|midnight| midnight.timestamp())
                .unwrap_or_else(|| now.timestamp() - i64::from(now.num_seconds_from_midnight())),
        }
    }
}

#[cfg(test)]
#[path = "tests/period.rs"]
mod tests;
