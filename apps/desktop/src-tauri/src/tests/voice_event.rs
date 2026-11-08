use super::*;
use crate::beep::{Asking, Field};
use serde_json::json;

#[test]
fn the_orb_reads_an_approval_and_its_answer() {
    let waiting = Waiting {
        session: "chat".into(),
        approval: Asking {
            query_id: "q1".into(),
            app: "Fastmail".into(),
            action: "Send email".into(),
            fields: vec![Field {
                label: "To".into(),
                value: "Alex".into(),
            }],
        },
        work: false,
    };
    let asked = serde_json::to_value(LiveEvent::Approval(waiting)).expect("it serializes");
    assert_eq!(
        asked,
        json!({
            "kind": "approval",
            "session": "chat",
            "approval": {
                "query_id": "q1",
                "app": "Fastmail",
                "action": "Send email",
                "fields": [{ "label": "To", "value": "Alex" }],
            },
            "work": false,
        })
    );
    let answered = LiveEvent::Answered {
        session: "chat".into(),
        query_id: "q1".into(),
        run: "r1".into(),
        decision: Decision::Always,
    };
    assert_eq!(
        serde_json::to_value(answered).expect("it serializes"),
        json!({ "kind": "answered", "session": "chat", "query_id": "q1", "run": "r1", "decision": "always" })
    );
}

#[test]
fn a_sent_message_names_its_run_for_the_bar() {
    let sent = LiveEvent::Sent {
        session: "chat".into(),
        text: "hi".into(),
        run: "r1".into(),
    };
    assert_eq!(
        serde_json::to_value(sent).expect("it serializes"),
        json!({ "kind": "sent", "session": "chat", "text": "hi", "run": "r1" })
    );
}
