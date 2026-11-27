use super::*;
use beep_proto::{AgentEntry, AgentEntryKind, AgentQuestion, MachineSession};
use std::time::SystemTime;

fn pushed(kind: UpdateKind, earlier: u32, texts: &[&str], status: AgentStatus) -> Pushed {
    let entries: Vec<_> = texts
        .iter()
        .map(|text| AgentEntry {
            kind: AgentEntryKind::Agent {},
            text: (*text).to_owned(),
            tool: None,
            sent_at: None,
        })
        .collect();
    let next = u64::from(earlier) + entries.len() as u64;
    Pushed {
        kind,
        cursor: SessionCursor {
            start: 0,
            next,
            fingerprint: next.to_string(),
            prefix_next: 0,
            prefix_fingerprint: String::new(),
        },
        resumed: false,
        view: AgentSessionView {
            session: MachineSession {
                reply: None,
                changes: None,
                machine_id: "build-box".into(),
                machine_name: "build-box".into(),
                id: "one".into(),
                agent: "codex".into(),
                title: None,
                cwd: "/srv/project".into(),
                status,
                doing: None,
                started_at: SystemTime::UNIX_EPOCH,
                last_activity: SystemTime::UNIX_EPOCH,
                external: false,
                question: None,
                subagents: 0,
                model: None,
                assistant: false,
                account: None,
                persona: None,
                place: None,
                continues: None,
            },
            plan: Vec::new(),
            files: Vec::new(),
            last_message: String::new(),
            error: None,
            queued: 0,
            turns: 0,
            entries,
            earlier,
            queued_messages: Vec::new(),
            model: None,
            last_stop: None,
            usage: None,
        },
        modes: None,
        tasks: Vec::new(),
    }
}

const WORKING: AgentStatus = AgentStatus::Working {};

fn texts(pushed: &Pushed) -> Vec<&str> {
    pushed
        .view
        .entries
        .iter()
        .map(|entry| entry.text.as_str())
        .collect()
}

#[test]
fn off_screen_deltas_go_once_a_second_with_every_entry() {
    let mut pace = Pace::default();
    let start = Instant::now();
    let first = pace.push(
        pushed(UpdateKind::Snapshot, 0, &["a"], WORKING),
        false,
        start,
    );
    assert_eq!(first.len(), 1);

    for (at, text) in ["b", "c", "d"].into_iter().enumerate() {
        let now = start + Duration::from_millis(40 * (at as u64 + 1));
        let delta = pushed(UpdateKind::Delta, 1 + at as u32, &[text], WORKING);
        assert!(pace.push(delta, false, now).is_empty());
    }

    // A delta that rewrites the last entry replaces it in place.
    let rewrite = pushed(UpdateKind::Delta, 3, &["d2", "e"], WORKING);
    assert!(
        pace.push(rewrite, false, start + Duration::from_millis(200))
            .is_empty()
    );
    assert_eq!(pace.due(), Some(start + OFF_SCREEN_EVERY));
    let held = pace.flush(start + OFF_SCREEN_EVERY);
    let held = held.as_ref().map(texts);
    assert_eq!(held, Some(vec!["b", "c", "d2", "e"]));
    assert_eq!(pace.due(), None);
}

#[test]
fn held_entries_join_a_snapshot_without_a_gap() {
    let mut pace = Pace::default();
    let start = Instant::now();
    pace.push(pushed(UpdateKind::Delta, 0, &["a"], WORKING), false, start);
    pace.push(
        pushed(UpdateKind::Snapshot, 0, &["x", "y"], WORKING),
        false,
        start + Duration::from_millis(10),
    );
    pace.push(
        pushed(UpdateKind::Delta, 2, &["z"], WORKING),
        false,
        start + Duration::from_millis(20),
    );
    let held = pace.flush(start + OFF_SCREEN_EVERY);
    assert!(
        held.as_ref()
            .is_some_and(|held| matches!(held.kind, UpdateKind::Snapshot))
    );
    assert_eq!(held.as_ref().map(texts), Some(vec!["x", "y", "z"]));
    assert_eq!(held.map(|held| held.cursor.next), Some(3));
}

#[test]
fn a_delta_past_what_is_held_sends_the_held_first() {
    let mut pace = Pace::default();
    let start = Instant::now();
    pace.push(pushed(UpdateKind::Delta, 0, &["a"], WORKING), false, start);
    pace.push(pushed(UpdateKind::Delta, 1, &["b"], WORKING), false, start);
    let out = pace.push(pushed(UpdateKind::Delta, 5, &["f"], WORKING), false, start);
    assert_eq!(out.iter().map(texts).collect::<Vec<_>>(), vec![vec!["b"]]);
    assert_eq!(pace.flush(start).as_ref().map(texts), Some(vec!["f"]));
}

#[test]
fn a_question_a_run_ending_or_the_screen_sends_at_once() {
    let mut pace = Pace::default();
    let start = Instant::now();
    pace.push(pushed(UpdateKind::Delta, 0, &["a"], WORKING), false, start);
    let mut asking = pushed(UpdateKind::Delta, 1, &["b"], AgentStatus::Asking {});
    asking.view.session.question = Some(AgentQuestion {
        id: 7,
        text: "Run tests".into(),
        detail: None,
        options: Vec::new(),
    });
    assert_eq!(pace.push(asking, false, start).len(), 1);
    let ended = pushed(UpdateKind::Delta, 2, &["c"], AgentStatus::Idle {});
    assert_eq!(pace.push(ended, false, start).len(), 1);
    let idle = pushed(UpdateKind::Delta, 3, &["d"], AgentStatus::Idle {});
    assert!(pace.push(idle, false, start).is_empty());
    let shown = pushed(UpdateKind::Delta, 4, &["e"], AgentStatus::Idle {});
    let out = pace.push(shown, true, start);
    assert_eq!(
        out.iter().map(texts).collect::<Vec<_>>(),
        vec![vec!["d", "e"]]
    );
}

#[test]
fn a_hidden_window_hears_nothing_until_it_shows() {
    let mut pace = Pace::default();
    let start = Instant::now();
    assert!(pace.window(true, start).is_none());

    let first = pace.push(
        pushed(UpdateKind::Snapshot, 0, &["a"], WORKING),
        true,
        start,
    );
    assert!(first.is_empty(), "even the session it showed waits");

    let ended = pushed(UpdateKind::Delta, 1, &["b"], AgentStatus::Ended {});
    let later = start + Duration::from_secs(5);
    assert!(
        pace.push(ended, false, later).is_empty(),
        "a run ending waits too"
    );
    assert_eq!(pace.due(), None, "nothing is due while hidden");

    let shown = pace
        .window(false, later)
        .expect("the held state goes on show");
    assert_eq!(texts(&shown), ["a", "b"]);
    assert!(pace.window(false, later).is_none());
}

#[test]
fn a_hidden_window_holds_only_what_the_cursor_covers() {
    let mut pace = Pace::default();
    let start = Instant::now();
    pace.window(true, start);
    pace.push(
        pushed(UpdateKind::Snapshot, 0, &["a"], WORKING),
        false,
        start,
    );

    for at in 1..200u32 {
        let mut delta = pushed(UpdateKind::Delta, at, &["x"], WORKING);
        delta.cursor.start = u64::from(at.saturating_sub(2));
        assert!(pace.push(delta, false, start).is_empty());
    }

    let shown = pace
        .window(false, start)
        .expect("the held state goes on show");
    assert_eq!(shown.view.earlier, 197);
    assert_eq!(texts(&shown), ["x", "x", "x"]);
}
