use super::*;
use serde_json::Value;

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
fn the_terminals_view_rides_a_keyed_topic_under_the_node_topic_a_window_follows() {
    assert_eq!(affected("terminals"), ["terminals"]);
    assert!(affected("*").contains(&"terminals"));
    assert_eq!(commands("terminals"), ["terminals_all"]);

    let views = views_topic("terminals");
    let followed = Topic::new("terminals");
    // The node's own snapshots of `terminals` are held under another key.
    assert_ne!(views, followed);
    assert!(followed.matches(&views));
    // Other views keep the topic named for them.
    assert_eq!(views_topic("machines"), Topic::new("machines"));
}

#[test]
fn progress_draws_at_most_once_a_gap_and_the_end_of_a_transfer_draws_at_once() {
    let start = Instant::now();
    let mut pace = DrawPace::default();
    assert_eq!(pace.next(start, SYNC_GAP, true), Pace::Now);
    pace.drawn = Some(start);

    let soon = start + SYNC_GAP / 2;
    assert_eq!(pace.next(soon, SYNC_GAP, true), Pace::At(start + SYNC_GAP));
    assert_eq!(pace.next(soon, SYNC_GAP, true), Pace::Waiting);
    assert_eq!(pace.next(soon, SYNC_GAP, false), Pace::Now);

    pace.waiting = false;
    assert_eq!(pace.next(start + SYNC_GAP, SYNC_GAP, true), Pace::Now);
}

#[test]
fn session_changes_pace_home_and_other_views_do_not() {
    let views = Views::default();
    assert_eq!(views.pace("home").map(|(_, gap)| gap), Some(HOME_GAP));
    assert_eq!(views.pace("sync").map(|(_, gap)| gap), Some(SYNC_GAP));
    assert!(views.pace("machines").is_none());

    for topic in ["agents", "agent"] {
        assert_eq!(affected(topic), ["home"], "{topic} draws only Home");
    }

    // The moves' progress is a view of its own, drawn beside Home.
    assert_eq!(affected("moves"), ["moves"]);
    assert_eq!(views_topic("moves"), Topic::keyed("moves", "views"));
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

#[test]
fn each_topic_draws_on_its_own_and_once_at_a_time() {
    let mut pending = Pending::default();
    pending.topics.extend(["accounts", "home"]);
    assert_eq!(
        pending.due(),
        Due {
            topics: vec!["accounts", "home"],
            projects: false,
        }
    );

    // Home changes while both draw: it waits for its own draw, which goes again.
    pending.topics.insert("home");
    assert_eq!(pending.due(), Due::default());
    assert!(pending.again("home"));
    assert!(!pending.again("home"));

    // Accounts end later and do not draw again; Home can start at once next time.
    assert!(!pending.again("accounts"));
    pending.topics.insert("home");
    assert_eq!(pending.due().topics, ["home"]);
}

#[test]
fn view_tasks_can_start_outside_a_tokio_runtime() {
    assert!(tokio::runtime::Handle::try_current().is_err());
    let views = Views::default();
    let (sent, received) = tokio::sync::oneshot::channel();

    views.spawn(async move {
        // Paced draws need the runtime's timer driver as well as its executor.
        tokio::time::sleep_until(tokio::time::Instant::now()).await;
        sent.send(()).unwrap();
    });

    tauri::async_runtime::block_on(async {
        tokio::time::timeout(Duration::from_secs(5), received)
            .await
            .unwrap()
            .unwrap();
    });
}

#[test]
fn the_hosts_are_drawn_once_and_redrawn_when_accounts_change() {
    assert!(commands("machines").contains(&"agent_hosts"));
    assert!(!commands("accounts").contains(&"agent_hosts"));
    assert!(affected("accounts").contains(&"machines"));
}

#[test]
fn the_slow_reads_of_the_machines_are_a_draw_of_their_own() {
    assert!(!commands("machines").contains(&"agent_harnesses"));
    assert!(affected("machines").contains(&"machine-details"));
    assert!(affected("accounts").contains(&"machine-details"));
    assert_eq!(followed("machine-details"), Topic::new("machines"));
    assert!(followed("machine-details").matches(&views_topic("machine-details")));
    assert_ne!(views_topic("machine-details"), views_topic("machines"));
}

#[test]
fn a_project_that_stops_copying_is_pushed_once_more_so_home_shows_how_it_ended() {
    let views = Views::default();
    let set = |names: &[&str]| names.iter().map(|name| name.to_string()).collect::<BTreeSet<_>>();

    assert_eq!(views.shown_unsettled(set(&["acme/widget"])), set(&["acme/widget"]));
    assert_eq!(views.shown_unsettled(set(&["acme/gadget"])), set(&["acme/gadget", "acme/widget"]));
    assert_eq!(views.shown_unsettled(set(&[])), set(&["acme/gadget"]));
    assert_eq!(views.shown_unsettled(set(&[])), set(&[]));
}
