use super::*;
use std::time::{Duration, UNIX_EPOCH};

#[test]
fn a_version_carries_its_time_in_milliseconds() {
    let row = VersionRow::from(InstructionsVersion {
        version: 7,
        text: "- Write plainly.".into(),
        note: String::new(),
        saved_at: UNIX_EPOCH + Duration::from_millis(1_500),
    });
    assert_eq!(row.saved_at, 1_500);
    assert_eq!(row.version, 7);
}
