use super::*;

#[test]
fn a_local_time_is_read_in_the_members_timezone() {
    let tz: Tz = "Asia/Tokyo".parse().expect("known timezone");
    let at = local_time("2026-10-05T09:30", tz).expect("parses");
    assert_eq!(at.to_rfc3339(), "2026-10-05T00:30:00+00:00");
    let at = local_time("2026-10-05T09:30:00+02:00", tz).expect("parses");
    assert_eq!(at.to_rfc3339(), "2026-10-05T07:30:00+00:00");
    assert!(local_time("tomorrow at 9", tz).is_none());
}
