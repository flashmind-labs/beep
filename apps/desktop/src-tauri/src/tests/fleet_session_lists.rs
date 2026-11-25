use super::*;
use beep_proto::AgentStatus;
use iroh::SecretKey;
use std::time::SystemTime;

fn machine(seed: u8) -> EndpointId {
    SecretKey::from_bytes(&[seed; 32]).public()
}

fn session(id: &str) -> MachineSession {
    MachineSession {
        reply: None,
        changes: None,
        machine_id: "build-box".into(),
        machine_name: "build-box".into(),
        id: id.into(),
        agent: "codex".into(),
        title: None,
        cwd: "/srv/project".into(),
        status: AgentStatus::Idle {},
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
    }
}

fn ids(rows: &[MachineSession]) -> Vec<String> {
    rows.iter().map(|row| row.id.to_string()).collect()
}

fn keys(moved: &[Asked]) -> Vec<MachineFolders> {
    moved.iter().map(|asked| asked.key.clone()).collect()
}

/// A list waits for every machine, sends once per change, and keeps a failing machine's rows.
#[test]
fn a_list_holds_until_every_machine_answered_and_keeps_rows_on_failure() {
    let me = machine(1);
    let other = machine(2);
    let local = (me, vec!["/srv/project".to_owned()]);
    let remote = (other, vec!["/srv/project".to_owned()]);
    let list = ListId::Project("project".into());
    let keep = |_: &[MachineFolders], rows: Vec<MachineSession>| rows;
    let mut lists = Lists::new(me);
    let now = Instant::now();

    let (rows, moved) = lists.want(
        list.clone(),
        "main",
        vec![local.clone(), remote.clone()],
        now,
        keep,
    );
    assert!(rows.is_none());
    assert_eq!(keys(&moved.fetch), [local.clone(), remote.clone()]);
    let (mine, theirs) = (moved.fetch[0].clone(), moved.fetch[1].clone());

    // This Mac answers first: nothing is sent while the other machine is still asked.
    assert!(
        lists
            .answered(mine, Some(vec![session("mine")]), now, keep)
            .sent
            .is_empty()
    );
    let sent = lists
        .answered(theirs, Some(vec![session("theirs")]), now, keep)
        .sent;
    assert_eq!(sent.len(), 1);
    assert_eq!(ids(&sent[0].1), ["mine", "theirs"]);

    // A read asks this Mac again, not the fresh machine, and gets the whole list.
    let (rows, moved) = lists.want(list, "main", vec![local.clone(), remote.clone()], now, keep);
    assert_eq!(keys(&moved.fetch), [local]);
    assert_eq!(ids(&rows.unwrap_or_default()), ["mine", "theirs"]);

    // The other machine fails: its rows stay and nothing changed, so nothing is sent.
    let moved = lists.stale(other);
    assert_eq!(keys(&moved.fetch), [remote]);
    assert!(
        lists
            .answered(moved.fetch[0].clone(), None, now, keep)
            .sent
            .is_empty()
    );

    let sent = lists.forget(other, &"theirs".into(), keep).sent;
    assert_eq!(sent.len(), 1);
    assert_eq!(ids(&sent[0].1), ["mine"]);
}

/// An answer asked before a session was deleted is dropped and the machine asked again.
#[test]
fn an_answer_from_before_a_delete_is_asked_again() {
    let me = machine(1);
    let other = machine(2);
    let remote = (other, vec!["/srv/project".to_owned()]);
    let keep = |_: &[MachineFolders], rows: Vec<MachineSession>| rows;
    let mut lists = Lists::new(me);
    let now = Instant::now();

    let (_, moved) = lists.want(ListId::Home, "main", vec![remote.clone()], now, keep);
    let first = moved.fetch[0].clone();
    let sent = lists
        .answered(
            first,
            Some(vec![session("kept"), session("gone")]),
            now,
            keep,
        )
        .sent;
    assert_eq!(ids(&sent[0].1), ["kept", "gone"]);

    let moved = lists.stale(other);
    let before = moved.fetch[0].clone();
    assert_eq!(
        ids(&lists.forget(other, &"gone".into(), keep).sent[0].1),
        ["kept"]
    );

    let moved = lists.answered(
        before,
        Some(vec![session("kept"), session("gone")]),
        now,
        keep,
    );
    assert!(moved.sent.is_empty());
    assert_eq!(keys(&moved.fetch), [remote]);
}

/// A move asks again only the answers that hold the session or cover its new folder.
#[test]
fn a_move_asks_only_the_answers_it_can_change() {
    let me = machine(1);
    let other = machine(2);
    let holds = (other, vec!["/srv/old".to_owned()]);
    let target = (other, vec!["/srv/new".to_owned()]);
    let apart = (other, vec!["/srv/apart".to_owned()]);
    let keep = |_: &[MachineFolders], rows: Vec<MachineSession>| rows;
    let mut lists = Lists::new(me);
    let now = Instant::now();

    for key in [&holds, &target, &apart] {
        let (_, moved) = lists.want(ListId::Home, "main", vec![key.clone()], now, keep);
        lists.answered(
            moved.fetch[0].clone(),
            Some(if *key == holds {
                vec![session("moving")]
            } else {
                Vec::new()
            }),
            now,
            keep,
        );
    }
    // Home holds one list at a time here: list all three.
    lists.want(
        ListId::Home,
        "main",
        vec![holds.clone(), target.clone(), apart],
        now,
        keep,
    );

    let moved = lists.relocated(other, &"moving".into(), "/srv/new");
    let asked = keys(&moved.fetch);
    assert_eq!(asked.len(), 2);
    assert!(asked.contains(&holds) && asked.contains(&target));
}

/// A list is dropped when the last window showing it lets go, never while one still shows it,
/// however long that is. Home stays.
#[test]
fn a_list_goes_with_its_last_window_and_never_while_one_shows_it() {
    let me = machine(1);
    let other = machine(2);
    let key = |folder: &str| (other, vec![folder.to_owned()]);
    let keep = |_: &[MachineFolders], rows: Vec<MachineSession>| rows;
    let mut lists = Lists::new(me);
    let now = Instant::now();
    let page = ListId::Project("acme/widget".into());

    let (_, moved) = lists.want(page.clone(), "main", vec![key("/srv/a")], now, keep);
    let asked = moved.fetch[0].clone();
    lists.drop_list(&page, "main");
    assert!(lists.answers.is_empty());
    // The answer that was out comes back to nothing.
    let moved = lists.answered(asked, Some(vec![session("late")]), now, keep);
    assert!(moved.sent.is_empty() && moved.fetch.is_empty());
    assert!(lists.answers.is_empty());

    // Two windows show it: one letting go, or closing, leaves the list to the other.
    lists.want(page.clone(), "main", vec![key("/srv/a")], now, keep);
    lists.want(page.clone(), "second", vec![key("/srv/a")], now, keep);
    lists.drop_list(&page, "main");
    assert_eq!(lists.lists.len(), 1);
    lists.closed("main");
    assert_eq!(lists.lists.len(), 1);
    lists.closed("second");
    assert!(lists.lists.is_empty() && lists.answers.is_empty());

    // An open page keeps its list without asking again, and so does Home.
    lists.want(page, "main", vec![key("/srv/a")], now, keep);
    lists.want(ListId::Home, "main", vec![key("/srv/b")], now, keep);
    let later = now + Duration::from_secs(24 * 3600);
    lists.want(ListId::Home, "main", vec![key("/srv/b")], later, keep);
    assert_eq!(lists.lists.len(), 2);
    lists.closed("main");
    assert_eq!(lists.lists.len(), 1);
    assert_eq!(lists.answers.len(), 1);
}

/// A project's Beep rows are the mirror's: its status now, and the sessions only it has.
#[test]
fn beep_rows_come_from_the_mirror_over_the_answers() {
    let other = machine(2);
    let key = (other, vec!["/srv/project".to_owned()]);
    let held = |id: &str, status: AgentStatus, cwd: &str| MachineSession {
        machine_id: other.to_string(),
        status,
        cwd: cwd.into(),
        ..session(id)
    };
    let mirrored = Rows::kept(vec![
        held("busy", AgentStatus::Working {}, "/srv/project"),
        held("new", AgentStatus::Working {}, "/srv/project/sub"),
        held("elsewhere", AgentStatus::Working {}, "/srv/other"),
    ]);
    let answered = vec![
        held("busy", AgentStatus::Idle {}, "/srv/project"),
        MachineSession {
            external: true,
            ..held("outside", AgentStatus::Ended {}, "/srv/project")
        },
    ];

    let rows = with_mirrored(answered, &mirrored, &[key]);
    let status = |id: &str| rows.iter().find(|row| row.id == id).map(|row| row.status);

    assert_eq!(status("busy"), Some(AgentStatus::Working {}));
    assert_eq!(status("outside"), Some(AgentStatus::Ended {}));
    assert!(status("new").is_some());
    assert!(status("elsewhere").is_none());
}

/// Only the machines whose Beep rows differ make their lists draw again.
#[test]
fn only_a_machine_whose_rows_changed_redraws_its_lists() {
    let (a, b) = (machine(2), machine(3));
    let held = |machine: EndpointId, status: AgentStatus| MachineSession {
        machine_id: machine.to_string(),
        status,
        ..session("s1")
    };
    let before = Rows::kept(vec![
        held(a, AgentStatus::Working {}),
        held(b, AgentStatus::Working {}),
    ]);
    let after = Rows::kept(vec![
        held(a, AgentStatus::Idle {}),
        held(b, AgentStatus::Working {}),
    ]);

    assert_eq!(after.changed_since(&before), HashSet::from([a]));
    assert!(after.changed_since(&after).is_empty());
}
