use super::*;
use beep_proto::AgentQuestion;

fn asking(question: u32) -> NeedsYou {
    NeedsYou::Agent {
        machine_id: "m1".into(),
        machine_name: "build-box".into(),
        agent: "claude".into(),
        session: "s1".into(),
        title: Some("Fix tests".into()),
        question: AgentQuestion {
            id: question,
            text: "Run cargo test".into(),
            detail: None,
            options: Vec::new(),
        },
    }
}

#[test]
fn agent_questions_open_their_session() {
    let listed = entries(vec![asking(3)]);
    assert_eq!(listed[0].id, "wait:agent:m1:s1:3");
    assert_eq!(listed[0].label, "Run cargo test");
    assert_eq!(
        listed[0].target,
        CenterTarget::Agent(AgentTarget::asking(
            "m1",
            "build-box",
            "claude",
            &SessionId::from("s1"),
            Some("Fix tests")
        ))
    );
}

#[test]
fn long_labels_are_cut_to_one_short_line() {
    let long = format!("Run\n{}", "x".repeat(100));
    let label = shorten(&long);
    assert_eq!(label.chars().count(), LABEL_MAX);
    assert!(label.starts_with("Run x"));
    assert!(label.ends_with('…'));
    assert_eq!(shorten("  short   one "), "short one");
}

#[test]
fn badge_shows_only_while_something_waits() {
    assert_eq!(badge(0), None);
    assert_eq!(badge(3), Some("3".into()));
    assert_eq!(tooltip(0), "Beep");
    assert_eq!(tooltip(1), "Beep: 1 waits on you");
    assert_eq!(tooltip(2), "Beep: 2 wait on you");
}

#[test]
fn the_list_says_when_it_changed() {
    let list = WaitingList::default();
    let one = entries(vec![asking(1)]);
    assert!(list.set(one.clone()));
    assert!(!list.set(one));
    assert!(list.target("wait:agent:m1:s1:1").is_some());
    assert!(list.target("wait:agent:m1:s1:2").is_none());
    assert!(list.set(Vec::new()));
}
