use super::*;

#[test]
fn failed_views_replace_values_and_successful_null_is_still_a_value() {
    let mut views = Vec::new();
    append::<Value>(
        &mut views,
        "center_home",
        Err(crate::error::DesktopError::BadRequest(
            "temporary source failure".into(),
        )),
    );
    assert_eq!(
        serde_json::to_value(&views[0]).unwrap(),
        json!({
            "command": "center_home", "args": {}, "error": "temporary source failure",
        })
    );
    let recovered = View::new("center_home", json!({}), Value::Null).unwrap();
    assert_eq!(
        serde_json::to_value(recovered).unwrap(),
        json!({
            "command": "center_home", "args": {}, "value": null,
        })
    );
}

#[test]
fn processes_do_not_draw_home_again() {
    assert!(affected("processes").is_empty());
    assert_eq!(affected("agent"), ["home"]);
}

#[test]
fn progress_draws_at_most_once_a_gap_and_the_end_of_a_transfer_draws_at_once() {
    let start = Instant::now();
    let mut pace = SyncPace::default();
    assert_eq!(pace.next(start, true), Pace::Now);
    pace.drawn = Some(start);

    let soon = start + SYNC_GAP / 2;
    assert_eq!(pace.next(soon, true), Pace::At(start + SYNC_GAP));
    assert_eq!(pace.next(soon, true), Pace::Waiting);
    assert_eq!(pace.next(soon, false), Pace::Now);

    pace.waiting = false;
    assert_eq!(pace.next(start + SYNC_GAP, true), Pace::Now);
}

#[test]
fn only_a_status_with_files_in_transit_is_moving() {
    use beep_proto::node::{
        events::Value,
        sync::{Progress, ProjectStatus, Reply},
    };

    let status = |progress| {
        Value::Sync(Box::new(Reply::Status {
            projects: vec![ProjectStatus {
                name: "acme/widget".into(),
                progress,
                ..ProjectStatus::default()
            }],
        }))
    };
    assert!(moving(&status(Some(Progress::default()))));
    assert!(!moving(&status(None)));
    assert!(!moving(&Value::Missing));
}
