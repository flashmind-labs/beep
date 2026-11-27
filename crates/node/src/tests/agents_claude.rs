use super::*;

#[test]
fn only_a_stopped_session_is_taken_over() {
    let mut found = ExternalSession {
        id: "0f3c2a9e-1111-4222-8333-944455556666".into(),
        cwd: "/".into(),
        presence: Presence::Working,
        pid: Some(41),
        ..ExternalSession::default()
    };
    let refused = refuse_takeover(&found).unwrap();
    assert!(refused.contains("can only follow it"), "{refused}");
    assert!(refused.contains("process 41"), "{refused}");
    found.presence = Presence::Recent;
    found.pid = None;
    let refused = refuse_takeover(&found).unwrap();
    assert!(refused.contains("was active a moment ago"), "{refused}");
    found.presence = Presence::Stopped;
    assert_eq!(refuse_takeover(&found), None);
    found.cwd = "/no/such/folder/here".into();
    assert_eq!(refuse_takeover(&found), None, "a gone folder is no reason");
}
