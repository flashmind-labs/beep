use super::*;

fn online(list: &[(u8, bool)]) -> HashMap<u8, bool> {
    list.iter().copied().collect()
}

#[test]
fn a_machine_offline_for_two_minutes_is_told_once_and_again_when_back() {
    let mut away = Away::default();
    let start = Instant::now();
    let at = |secs| start + Duration::from_secs(secs);
    assert!(away.step(&online(&[(1, true)]), at(0)).is_empty());
    assert!(away.step(&online(&[(1, false)]), at(10)).is_empty());
    assert!(away.step(&online(&[(1, false)]), at(100)).is_empty());
    assert_eq!(
        away.step(&online(&[(1, false)]), at(130)),
        [Change::Gone {
            id: 1,
            since: at(10)
        }]
    );
    assert!(away.step(&online(&[(1, false)]), at(400)).is_empty());
    assert_eq!(away.step(&online(&[(1, true)]), at(500)), [Change::Back(1)]);
    assert!(away.step(&online(&[(1, true)]), at(510)).is_empty());
}

#[test]
fn the_watch_wakes_when_a_machine_away_is_due() {
    let mut away = Away::default();
    let start = Instant::now();
    let at = |secs| start + Duration::from_secs(secs);
    away.step(&online(&[(1, true), (2, true)]), at(0));
    assert_eq!(away.due(), None, "nothing is away");

    away.step(&online(&[(1, false), (2, true)]), at(10));
    away.step(&online(&[(1, false), (2, false)]), at(20));
    assert_eq!(away.due(), Some(at(10) + AWAY));

    away.step(&online(&[(1, false), (2, false)]), at(10) + AWAY);
    assert_eq!(
        away.due(),
        Some(at(20) + AWAY),
        "a told machine waits no more"
    );
}

#[test]
fn reads_that_ask_the_machines_follow_changes_at_most_once_a_minute() {
    let mut slow = Slow::default();
    let start = Instant::now();
    assert!(slow.due(start), "the first read goes at once");

    slow.read(start);
    assert!(!slow.due(start + SLOW_GAP), "nothing changed");
    assert_eq!(slow.wake(), None, "idle, nothing wakes the bell");

    slow.wanted = true;
    assert!(
        !slow.due(start + Duration::from_secs(30)),
        "a change waits for the gap"
    );
    assert_eq!(slow.wake(), Some(start + SLOW_GAP));
    assert!(slow.due(start + SLOW_GAP));
}

#[test]
fn a_short_drop_says_nothing() {
    let mut away = Away::default();
    let start = Instant::now();
    let at = |secs| start + Duration::from_secs(secs);
    away.step(&online(&[(1, true)]), at(0));
    away.step(&online(&[(1, false)]), at(10));
    assert!(away.step(&online(&[(1, true)]), at(60)).is_empty());
    // The next drop counts from its own start.
    away.step(&online(&[(1, false)]), at(100));
    assert!(away.step(&online(&[(1, false)]), at(200)).is_empty());
}

#[test]
fn a_machine_never_seen_online_or_gone_from_the_mesh_says_nothing() {
    let mut away = Away::default();
    let start = Instant::now();
    let at = |secs| start + Duration::from_secs(secs);
    away.step(&online(&[(1, false), (2, true)]), at(0));
    assert!(
        away.step(&online(&[(1, false), (2, false)]), at(10))
            .is_empty()
    );
    assert!(away.step(&online(&[(1, false)]), at(300)).is_empty());
    assert!(
        away.step(&online(&[(1, false), (2, false)]), at(600))
            .is_empty()
    );
}

fn set(paths: &[&str]) -> BTreeSet<String> {
    paths.iter().map(|p| (*p).to_owned()).collect()
}

#[test]
fn conflicts_raise_on_a_new_path_and_settle_when_gone() {
    let mut conflicts = Conflicts::default();
    let none = BTreeSet::new;
    assert_eq!(conflicts.step("p", &set(&[]), none), Settle::Same);
    assert_eq!(conflicts.step("p", &set(&["a"]), none), Settle::Raise);
    assert_eq!(conflicts.step("p", &set(&["a"]), none), Settle::Same);
    assert_eq!(conflicts.step("p", &set(&["a", "b"]), none), Settle::Raise);
    // One settled of two is not news.
    assert_eq!(conflicts.step("p", &set(&["b"]), none), Settle::Same);
    assert_eq!(conflicts.step("p", &set(&[]), none), Settle::Settled);
    assert_eq!(conflicts.step("p", &set(&[]), none), Settle::Same);
}

#[test]
fn conflicts_a_kept_note_holds_are_not_raised_again_at_start() {
    let mut conflicts = Conflicts::default();
    assert_eq!(
        conflicts.step("p", &set(&["a"]), || set(&["a"])),
        Settle::Same
    );
    let mut fresh = Conflicts::default();
    assert_eq!(fresh.step("p", &set(&[]), || set(&["a"])), Settle::Settled);
}

#[test]
fn a_machine_s_pushed_sync_gives_its_conflicts_without_asking_anyone() {
    use beep_proto::node::{
        events::Value,
        sync::{ProjectStatus, Reply},
    };

    let looks = Looks::new();
    let machine = iroh::SecretKey::from_bytes(&[3; 32]).public();
    let pushed = |paths: &[&str]| {
        Value::Sync(Box::new(Reply::Status {
            projects: vec![ProjectStatus {
                name: "acme/widget".into(),
                conflicts: paths.iter().map(|path| path.to_string()).collect(),
                ..ProjectStatus::default()
            }],
        }))
    };

    looks.hear(machine, "sync", &pushed(&["a.txt"]));
    looks.hear(machine, "sync", &pushed(&["a.txt", "b.txt"]));
    let said = looks.take_said();
    assert_eq!(
        said[&machine],
        vec![("acme/widget".to_owned(), set(&["a.txt", "b.txt"]))]
    );
    assert!(looks.take_said().is_empty(), "taken once");

    // A change of the moves reads for a failed move; any other topic says nothing.
    assert!(!looks.moved.load(Ordering::Relaxed));
    looks.hear(machine, "agents", &Value::Missing);
    assert!(!looks.moved.load(Ordering::Relaxed));
    looks.hear(machine, "moves", &Value::Missing);
    assert!(looks.moved.load(Ordering::Relaxed));
}

#[test]
fn a_machine_s_pushed_moves_say_which_failed_and_a_machine_that_pushes_none_is_asked() {
    use beep_proto::{
        AgentSlug, SessionId,
        node::{
            agent::{MoveEntry, MovePhase, MoveProgress, SessionsReply},
            events::Value,
        },
    };

    let looks = Looks::new();
    let (to, other) = (
        iroh::SecretKey::from_bytes(&[4; 32]).public(),
        iroh::SecretKey::from_bytes(&[5; 32]).public(),
    );
    let record = SessionMove {
        agent: AgentSlug::from("codex"),
        id: SessionId::from("s1"),
        from: other,
        to,
        project: "acme/widget".into(),
        path: String::new(),
        mode: None,
        stop: false,
        at: 9,
        worktree: None,
        model: None,
        worktrees: Vec::new(),
        conversation: None,
    };
    let entry = |at, phase, reason: &str| {
        Value::Sessions(Box::new(SessionsReply::Moves {
            moves: vec![MoveEntry {
                session: SessionId::from("s1"),
                at,
                progress: MoveProgress {
                    phase,
                    reason: Some(reason.to_owned()),
                    ..MoveProgress::default()
                },
            }],
        }))
    };

    assert_eq!(heard_of(&record, &looks.pushed_moves(), 10), Heard::Silent);
    // A machine that pushes its moves needs no read of the moves ring.
    looks.hear(to, "move-progress", &entry(9, MovePhase::Files, ""));
    looks.hear(to, "moves", &Value::Missing);
    assert!(!looks.moved.load(Ordering::Relaxed));
    assert_eq!(heard_of(&record, &looks.pushed_moves(), 10), Heard::Going);

    // The outcome of an earlier move of the session is not this move's.
    looks.hear(to, "move-progress", &entry(8, MovePhase::Failed, "old"));
    assert_eq!(heard_of(&record, &looks.pushed_moves(), 10), Heard::Going);
    looks.hear(to, "move-progress", &entry(9, MovePhase::Failed, "no room"));
    assert_eq!(
        heard_of(&record, &looks.pushed_moves(), 10),
        Heard::Failed("no room".into())
    );
    looks.hear(to, "move-progress", &entry(9, MovePhase::Done, ""));
    assert_eq!(heard_of(&record, &looks.pushed_moves(), 10), Heard::Over);

    // An outcome the machine no longer holds, long after the move began, is asked for.
    looks.hear(to, "move-progress", &entry(8, MovePhase::Failed, "old"));
    assert_eq!(
        heard_of(&record, &looks.pushed_moves(), 9 + LOST_AFTER_MS + 1),
        Heard::Lost
    );
}
