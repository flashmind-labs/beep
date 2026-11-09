use super::*;

#[test]
fn signed_message_changes_with_body_and_time() {
    let a = signed_message(b"a", 10);
    let b = signed_message(b"b", 10);
    let later = signed_message(b"a", 11);
    assert_ne!(a, b);
    assert_ne!(a, later);
    assert_ne!(b, later);

    for message in [&a, &b, &later] {
        assert!(message.starts_with(b"beep-report/1\0"));
    }
}

#[test]
fn report_id_is_stable_and_short() {
    let id = report_id(b"body");
    assert_eq!(id, report_id(b"body"));
    assert_ne!(id, report_id(b"other"));
    assert_eq!(id.len(), 16);
    assert!(id.chars().all(|c| matches!(c, 'a'..='z' | '2'..='7')));
}
