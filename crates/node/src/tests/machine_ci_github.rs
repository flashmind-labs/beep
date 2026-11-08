use super::{View, changes};
use beep_proto::RunState;

fn view(text: &str) -> View {
    serde_json::from_str(text).expect("the fixture reads")
}

#[test]
fn only_what_changed_is_said_and_the_end_reads() {
    let first = view(
        r#"{"status":"in_progress","conclusion":"","name":"CI","headBranch":"main","url":"",
            "jobs":[{"name":"test","status":"in_progress","conclusion":"","steps":[]}]}"#,
    );
    assert_eq!(changes(None, &first), ["CI in_progress", "test started"]);
    assert!(changes(Some(&first), &first).is_empty());
    let ended = view(
        r#"{"status":"completed","conclusion":"failure","name":"CI","headBranch":"main","url":"",
            "jobs":[{"name":"test","status":"completed","conclusion":"failure",
            "steps":[{"name":"cargo test","conclusion":"failure"}]}]}"#,
    );
    assert_eq!(
        changes(Some(&first), &ended),
        ["test finished: failure", "test: cargo test failed"]
    );
    assert_eq!(first.state(), RunState::Running {});
    assert_eq!(ended.state(), RunState::Failed {});
}
