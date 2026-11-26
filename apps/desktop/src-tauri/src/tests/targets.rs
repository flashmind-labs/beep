use super::*;

fn key(question: Option<u32>) -> AgentNoticeKey {
    AgentNoticeKey {
        machine_id: "m1".into(),
        agent: "claude".into(),
        session: "s1".into(),
        question,
        machine_name: "build-box".into(),
    }
}

#[test]
fn a_notice_opens_its_agent_session() {
    let asking = AgentTarget::notified(&key(Some(2)));
    assert_eq!(
        asking,
        AgentTarget::asking("m1", "build-box", "claude", &SessionId::from("s1"), None)
    );
    let done = AgentTarget::notified(&key(None));
    assert_eq!(done.status, "idle");
    assert_eq!(done.id, "s1");
}

#[test]
fn a_project_window_hands_its_targets_to_the_main_window() {
    let read = |json: &str| serde_json::from_str::<CenterTarget>(json).ok();
    assert_eq!(
        read(r#"{"projects":{"project":"org/site"}}"#),
        Some(CenterTarget::Projects {
            project: Some("org/site".into()),
            run: None,
        })
    );
    assert_eq!(
        read(r#"{"projects":{"project":null}}"#),
        Some(CenterTarget::Projects {
            project: None,
            run: None
        })
    );
    assert_eq!(read(r#""terminals""#), Some(CenterTarget::Terminals));
    assert_eq!(read(r#""machines""#), Some(CenterTarget::Machines));
    assert_eq!(read(r#""report""#), Some(CenterTarget::Report));
    assert_eq!(read(r#""research""#), None);
}

#[test]
fn agent_targets_read_as_the_pane_takes_them() {
    let agent = CenterTarget::Agent(AgentTarget::asking(
        "m1",
        "Studio",
        "claude",
        &SessionId::from("s1"),
        None,
    ));
    let json = serde_json::to_value(&agent).expect("serializes");
    assert_eq!(json["agent"]["kind"], "agent");
    assert_eq!(json["agent"]["id"], "s1");
    assert_eq!(json["agent"]["status"], "asking");
    let back: CenterTarget = serde_json::from_value(json).expect("reads back");
    assert_eq!(back, agent);
}
