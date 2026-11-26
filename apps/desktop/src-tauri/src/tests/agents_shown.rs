use super::*;

fn key(session: &str, question: Option<u32>) -> AgentNoticeKey {
    AgentNoticeKey {
        machine_id: "m1".into(),
        agent: "claude".into(),
        session: session.into(),
        question,
        machine_name: "build-box".into(),
    }
}

#[test]
fn an_agent_session_counts_as_shown_while_its_view_reads_it() {
    let shown = AgentsShown::default();
    let at = Instant::now();
    shown.read("center", "m1", "claude", &SessionId::from("s1"), at);
    let s1 = key("s1", None);
    assert!(shown.shows(&["center"], &s1, at + Duration::from_secs(5)));
    assert!(
        !shown.shows(&["center"], &s1, at + AGENT_SHOWN_FOR),
        "a view that stopped reading has moved on"
    );
    assert!(
        !shown.shows(&["bar"], &s1, at),
        "only where it is on screen"
    );
    assert!(!shown.shows(&["center"], &key("s2", None), at));
    shown.clear("center");
    assert!(!shown.shows(&["center"], &s1, at));
}
