use super::*;
use beep_proto::{Approval, PendingApproval};

fn sample(session: &str, run: Option<&str>, group: bool) -> Notice {
    Notice {
        title: "Allow Fastmail: send email?".into(),
        body: "To Sam".into(),
        session_id: session.into(),
        app: None,
        approval: run.map(|run| PendingApproval {
            run_id: run.into(),
            query_id: "q1".into(),
            approval: Approval {
                app: "Fastmail".into(),
                action: "send email".into(),
                fields: Vec::new(),
            },
        }),
        changed: false,
        group,
    }
}

#[test]
fn a_click_opens_the_chat_or_group() {
    let id = Uuid::new_v4();
    assert_eq!(
        click_target(&sample(&id.to_string(), None, false)),
        CenterTarget::Chat { session: Some(id) }
    );
    assert_eq!(
        click_target(&sample("g1", None, true)),
        CenterTarget::Group { id: "g1".into() }
    );
    let mut lost = sample("", None, false);
    lost.app = Some("gmail".into());
    assert_eq!(click_target(&lost), CenterTarget::Apps);
    assert_eq!(click_target(&sample("", None, false)), CenterTarget::Home);
}

fn agent_notice(question: Option<u32>) -> (Notice, AgentNoticeKey) {
    let key = AgentNoticeKey {
        machine_id: "m1".into(),
        agent: "claude".into(),
        session: "s1".into(),
        question,
        machine_name: "build-box".into(),
    };
    let mut notice = sample(&key.to_string(), None, false);
    notice.title = "Claude Code on build-box asks".into();
    (notice, key)
}

#[test]
fn an_alert_opens_in_the_notifications_pane() {
    let id = Uuid::new_v4();
    let target = CenterTarget::Notice { session: Some(id) };
    assert_eq!(
        serde_json::to_value(&target).expect("serializes"),
        serde_json::json!({ "notice": { "session": id } })
    );
}

#[test]
fn an_agent_notice_opens_its_session_and_a_question_has_answers() {
    let (asks, key) = agent_notice(Some(4));
    assert_eq!(
        click_target(&asks),
        CenterTarget::Agent(AgentTarget::notified(&key))
    );
    assert_eq!(ask(&asks), Some(Ask::Agent(key)));
    let (done, key) = agent_notice(None);
    assert_eq!(
        click_target(&done),
        CenterTarget::Agent(AgentTarget::notified(&key))
    );
    assert_eq!(ask(&done), None, "a finished turn only opens");
    assert_eq!((agent_reply(true), agent_reply(false)), ("allow", "deny"));
}

#[test]
fn approvals_ask_whoever_waits_on_them() {
    let id = Uuid::new_v4().to_string();
    assert_eq!(ask(&sample(&id, None, false)), None);
    assert_eq!(
        ask(&sample(&id, Some("run1"), false)),
        Some(Ask::Turn {
            session: id.clone(),
            query_id: "q1".into()
        })
    );
    assert_eq!(
        ask(&sample(&id, Some(""), false)),
        Some(Ask::Work {
            session: id.clone(),
            query_id: "q1".into()
        })
    );
    assert_eq!(
        ask(&sample("g1", Some(""), true)),
        Some(Ask::Group {
            group: "g1".into(),
            query_id: "q1".into()
        })
    );
    let mut resolved = sample(&id, Some(""), false);
    resolved.changed = true;
    assert_eq!(ask(&resolved), None);
}
