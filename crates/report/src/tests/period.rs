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
