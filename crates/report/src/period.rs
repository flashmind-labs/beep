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
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(h: u32, m: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(2026, 11, 6, h, m, 0)
            .single()
            .expect("a fixed mid-day local time is unambiguous")
    }

    #[test]
    fn today_starts_at_local_midnight() {
        let t = at(9, 30);
        assert_eq!(Period::Today.since(t), at(0, 0).timestamp());
        assert_eq!(Period::Hour.since(t), t.timestamp() - 3600);
        assert_eq!(Period::Day.since(t), t.timestamp() - 86400);
    }

    #[test]
    fn today_just_after_midnight_still_covers_the_minutes_since() {
        let t = at(0, 5);
        assert_eq!(Period::Today.since(t), t.timestamp() - 300);
    }

    #[test]
    fn period_reads_as_the_cli_writes_it() {
        assert_eq!("hour".parse::<Period>(), Ok(Period::Hour));
        assert_eq!(<&str>::from(Period::Day), "day");
        assert!("week".parse::<Period>().is_err());
    }
}
