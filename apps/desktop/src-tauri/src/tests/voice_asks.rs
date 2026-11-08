use super::*;
use crate::beep::Asking;

fn waiting(session: &str, query_id: &str, work: bool) -> Waiting {
    Waiting {
        session: session.into(),
        approval: Asking {
            query_id: query_id.into(),
            app: "Slack".into(),
            action: "Send message".into(),
            fields: Vec::new(),
        },
        work,
    }
}

fn notice(session: &str) -> Notice {
    Notice {
        title: "Allow Slack: Send message?".into(),
        body: String::new(),
        session_id: session.into(),
        app: None,
        approval: None,
        changed: false,
        group: false,
    }
}

#[test]
fn the_stream_and_the_notice_of_one_approval_show_once() {
    let mut asks = Asks::default();
    assert!(asks.add(waiting("a", "q1", false), None));
    assert!(!asks.add(waiting("a", "q1", false), Some(notice("a"))));
    assert_eq!(asks.all().len(), 1);
    assert_eq!(asks.into_notices().len(), 1);
}

#[test]
fn an_answered_approval_is_not_notified_when_live_mode_ends() {
    let mut asks = Asks::default();
    asks.add(waiting("a", "q1", true), Some(notice("a")));
    asks.add(waiting("b", "q2", false), Some(notice("b")));
    assert!(asks.remove("q1").is_some());
    assert!(asks.remove("q1").is_none());
    let left = asks.into_notices();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].session_id, "b");
}

#[test]
fn a_new_message_drops_only_its_chats_turn_approvals() {
    let mut asks = Asks::default();
    asks.add(waiting("a", "turn", false), None);
    asks.add(waiting("a", "work", true), None);
    asks.add(waiting("b", "other", false), None);
    assert_eq!(asks.drop_turns_of("a"), vec!["turn".to_owned()]);
    let left: Vec<_> = asks
        .all()
        .into_iter()
        .map(|w| w.approval.query_id)
        .collect();
    assert_eq!(left, vec!["work".to_owned(), "other".to_owned()]);
}
