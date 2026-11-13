use super::*;

fn agent_notice(question: Option<u32>) -> (Notice, AgentNoticeKey) {
    let key = AgentNoticeKey {
        machine_id: "m1".into(),
        agent: "claude".into(),
        session: "s1".into(),
        question,
        machine_name: "build-box".into(),
    };
    let notice = Notice {
        title: "Claude Code on build-box asks".into(),
        body: "Run the tests?".into(),
        session_id: key.to_string(),
    };
    (notice, key)
}

#[test]
fn an_agent_notice_opens_its_session_and_a_question_has_answers() {
    let (asks, key) = agent_notice(Some(4));
    assert_eq!(
        click_target(&asks),
        CenterTarget::Agent(AgentTarget::notified(&key))
    );
    assert_eq!(ask(&asks), Some(key));
    let (done, key) = agent_notice(None);
    assert_eq!(
        click_target(&done),
        CenterTarget::Agent(AgentTarget::notified(&key))
    );
    assert_eq!(ask(&done), None, "a finished turn only opens");
    assert_eq!((agent_reply(true), agent_reply(false)), ("allow", "deny"));
}

#[test]
fn any_other_notice_opens_home() {
    let (mut notice, _) = agent_notice(None);
    notice.session_id = String::new();
    assert_eq!(click_target(&notice), CenterTarget::Home);
    assert_eq!(ask(&notice), None);
}
