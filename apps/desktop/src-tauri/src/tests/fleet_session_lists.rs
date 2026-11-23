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
    let keep = |rows: Vec<MachineSession>| rows;
    let mut lists = Lists::new(me);
    let now = Instant::now();

    let (rows, moved) = lists.want(list.clone(), vec![local.clone(), remote.clone()], now, keep);
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
    let (rows, moved) = lists.want(list, vec![local.clone(), remote.clone()], now, keep);
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
    let keep = |rows: Vec<MachineSession>| rows;
    let mut lists = Lists::new(me);
    let now = Instant::now();

    let (_, moved) = lists.want(ListId::Home, vec![remote.clone()], now, keep);
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
