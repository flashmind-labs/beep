use super::*;
use beep_proto::node::agent::Entry;

#[test]
fn validates_limits_and_keeps_the_earlier_page_cursor() {
    for value in [
        json!({"session":""}),
        json!({"session":"s", "last":0}),
        json!({"session":"s", "last":51}),
        json!({"session":"s", "path":"/srv/project"}),
    ] {
        assert!(Args::try_from(value).is_err());
    }
    let args = Args::try_from(json!({"session":"s", "machine":"build-box", "before":12})).unwrap();
    assert_eq!(
        args.request(),
        SessionsRequest::Transcript {
            session: "s".into(),
            last: 20,
            thoughts: false,
            before: Some(12)
        }
    );
}

#[test]
fn preserves_cursor_and_bounds_unicode_text_without_exposing_thoughts() {
    let entries = vec![
        Entry::new(EntryKind::User, "é".repeat(TEXT_BUDGET)),
        Entry::new(EntryKind::Thought, "private reasoning".into()),
        Entry::new(EntryKind::Agent, "reply".into()),
    ];
    let Reply::Done { text, failed } = render(SessionsReply::Transcript {
        entries,
        earlier: 7,
    })
    .unwrap() else {
        panic!("done")
    };
    assert!(!failed);
    let page: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(page["next_before"], 7);
    assert_eq!(page["entries"].as_array().unwrap().len(), 2);
    assert_eq!(page["entries"][0]["truncated"], true);
    assert!(page["entries"][0]["text"].as_str().unwrap().len() <= TEXT_BUDGET / 2);
    assert!(!text.contains("private reasoning"));
}

#[test]
fn missing_sessions_and_old_peers_are_errors_not_empty_context() {
    for read in [
        SessionsReply::Error {
            message: "session unavailable".into(),
        },
        SessionsReply::Sessions { sessions: vec![] },
    ] {
        assert!(matches!(
            render(read).unwrap(),
            Reply::Done { failed: true, .. }
        ));
    }
    let Reply::Done { text, .. } = render(SessionsReply::Transcript {
        entries: vec![],
        earlier: 0,
    })
    .unwrap() else {
        panic!("done")
    };
    assert_eq!(
        serde_json::from_str::<Value>(&text).unwrap()["next_before"],
        Value::Null
    );
}
