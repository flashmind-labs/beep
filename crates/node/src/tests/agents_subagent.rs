use super::*;
use serde_json::json;

#[test]
fn conversation_ids_are_bounded_store_ids_not_paths() {
    for id in ["subagent:codex:thread-1", "subagent:claude:parent:child"] {
        assert_eq!(
            Conversation::try_from(id).expect("a native id").to_string(),
            id
        );
    }

    for id in [
        "subagent:codex:../thread",
        "subagent:claude:parent:child:extra",
        "subagent:claude:parent:",
        "subagent:codex:/tmp/file",
    ] {
        assert_eq!(Conversation::try_from(id).ok(), None);
    }
}

#[test]
fn a_codex_spawn_names_its_threads_once_it_has_them() {
    let started = json!({"prompt": "sleep", "receiverThreadIds": []});
    let mut found = Subagent::of_call(CODEX_SPAWN, Some(&started)).expect("a subagent");
    assert!(found.threads.is_empty());
    assert_eq!(found.title.as_deref(), Some("sleep"));
    let done = json!({"prompt": "sleep", "receiverThreadIds": ["t1", " "]});
    found.update(Some(&done));
    found.update(Some(&done));
    assert_eq!(found.threads, ["t1"]);
    // Waiting on a subagent starts none.
    assert!(Subagent::of_call("wait", Some(&done)).is_none());
}

#[test]
fn spawn_calls_preserve_the_requested_model() {
    for (title, input) in [
        (
            "Agent",
            json!({"subagent_type": "general-purpose", "model": "sonnet"}),
        ),
        (
            CODEX_SPAWN,
            json!({"prompt": "Check files", "model": "sonnet"}),
        ),
    ] {
        let sub = Subagent::of_call(title, Some(&input)).expect("a spawn");
        assert_eq!(sub.model.as_deref(), Some("sonnet"));
    }
}
