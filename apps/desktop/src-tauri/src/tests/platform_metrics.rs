use super::*;

#[test]
fn rejects_unbounded_labels_and_invalid_durations() {
    let mut sample = Timing {
        kind: "page".into(),
        name: "agent".into(),
        outcome: "ok".into(),
        ms: 25.0,
    };
    assert_eq!(labels(&sample), Some(("page_mount_painted", "agent", "ok")));
    sample.name = "a private page argument".into();
    assert_eq!(labels(&sample), None);
    sample.name = "agent".into();
    sample.ms = f64::NAN;
    assert_eq!(labels(&sample), None);
    sample.ms = 600_001.0;
    assert_eq!(labels(&sample), None);
}
