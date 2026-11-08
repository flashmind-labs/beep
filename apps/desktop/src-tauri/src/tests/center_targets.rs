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
        AgentTarget::asking("m1", "build-box", "claude", "s1", None)
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
    assert_eq!(read(r#""research""#), Some(CenterTarget::Research));
    assert_eq!(read(r#""report""#), Some(CenterTarget::Report));
    let session = Uuid::new_v4();
    assert_eq!(
        read(&format!(r#"{{"open_research":{{"session":"{session}"}}}}"#)),
        Some(CenterTarget::OpenResearch { session })
    );
    assert_eq!(
        read(r#"{"ask":{"prompt":"hi"}}"#),
        Some(CenterTarget::Ask {
            prompt: "hi".into()
        })
    );
}

#[test]
fn target_reads_from_js() {
    let new_chat: CenterTarget =
        serde_json::from_str(r#"{"chat":{"session":null}}"#).expect("a new chat parses");
    assert_eq!(new_chat, CenterTarget::Chat { session: None });
    let memories: CenterTarget = serde_json::from_str(r#""memories""#).expect("parses");
    assert_eq!(memories, CenterTarget::Memories);
    let id = Uuid::new_v4();
    let chat: CenterTarget =
        serde_json::from_str(&format!(r#"{{"chat":{{"session":"{id}"}}}}"#)).expect("parses");
    assert_eq!(chat, CenterTarget::Chat { session: Some(id) });
}

#[test]
fn group_and_agent_targets_read_as_the_pane_takes_them() {
    let group = CenterTarget::Group { id: "g1".into() };
    assert_eq!(
        serde_json::to_value(&group).expect("serializes"),
        serde_json::json!({ "group": { "id": "g1" } })
    );
    let agent = CenterTarget::Agent(AgentTarget::asking("m1", "Studio", "claude", "s1", None));
    let json = serde_json::to_value(&agent).expect("serializes");
    assert_eq!(json["agent"]["kind"], "agent");
    assert_eq!(json["agent"]["id"], "s1");
    assert_eq!(json["agent"]["status"], "asking");
    let back: CenterTarget = serde_json::from_value(json).expect("reads back");
    assert_eq!(back, agent);
}

#[test]
fn pending_is_taken_once() {
    let pending = Pending::default();
    pending.put(CenterTarget::Chats);
    assert_eq!(pending.take(), Some(CenterTarget::Chats));
    assert_eq!(pending.take(), None);
}
