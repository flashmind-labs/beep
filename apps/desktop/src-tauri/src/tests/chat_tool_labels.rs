use super::*;

#[test]
fn memory_results_name_their_action() {
    assert_eq!(action_label(MEMORY, "[]"), Some("Recalling memories"));
    assert_eq!(
        action_label(
            MEMORY,
            r#"[{"id":"m1","content":"Tea","replaced":"2026-01-01"}]"#
        ),
        Some("Checking a memory's history")
    );
    assert_eq!(
        action_label(MEMORY, r#"{"stored":true,"id":"m1","content":"Tea"}"#),
        Some("Remembering")
    );
    assert_eq!(
        action_label(MEMORY, r#"{"forgotten":1}"#),
        Some("Forgetting a memory")
    );
}

#[test]
fn background_and_skill_results_name_their_action() {
    assert_eq!(
        action_label(
            BACKGROUND,
            "Started agent w3. Its report comes back to this chat."
        ),
        Some("Starting an agent")
    );
    assert_eq!(action_label(BACKGROUND, "w3 finished:\nDone"), None);
    assert_eq!(
        action_label("skill", r#"{"name":"Padel","description":"","body":"Ask"}"#),
        Some("Loading a skill")
    );
}

#[test]
fn other_tools_and_failures_have_none() {
    assert_eq!(action_label("web_search", r#"{"stored":true}"#), None);
    assert_eq!(
        action_label(MEMORY, "memory store is not available here."),
        None
    );
}
