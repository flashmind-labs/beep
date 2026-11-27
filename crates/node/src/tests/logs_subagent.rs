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
    let mut found = Subagent::of_call(None, CODEX_SPAWN, Some(&started)).expect("a subagent");
    assert!(found.threads.is_empty());
    assert_eq!(found.title.as_deref(), Some("sleep"));
    let done = json!({"prompt": "sleep", "receiverThreadIds": ["t1", " "]});
    found.update(Some(&done));
    found.update(Some(&done));
    assert_eq!(found.threads, ["t1"]);
    // Waiting on a subagent starts none.
    assert!(Subagent::of_call(None, "wait", Some(&done)).is_none());
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
        let sub = Subagent::of_call(None, title, Some(&input)).expect("a spawn");
        assert_eq!(sub.model.as_deref(), Some("sonnet"));
    }
}

#[test]
fn flsh_agent_events_use_the_existing_card_without_claiming_other_tools() {
    let input = json!({"flshAgent": true, "name": "worker", "model": "provider:model"});
    let sub = Subagent::of_call(Some("flsh_agent"), "Check files", Some(&input))
        .expect("a flsh agent");
    assert_eq!(sub.name.as_deref(), Some("worker"));
    assert_eq!(sub.model.as_deref(), Some("provider:model"));
    assert!(!sub.codex);
    assert!(Subagent::of_call(Some("agent_wait"), "Waiting", Some(&input)).is_none());
    assert!(Subagent::of_call(Some("flsh_agent"), "Check files", Some(&json!({}))).is_none());
}

#[test]
fn a_pi_subagent_call_starts_one_unless_it_manages_runs() {
    let single = json!({"agent": "scout", "task": "\nFind the config\nthen report", "model": "anthropic/sonnet"});
    let sub = Subagent::of_call(None, PI_SPAWN, Some(&single)).expect("a pi subagent");
    assert_eq!(sub.name.as_deref(), Some("scout"));
    assert_eq!(sub.title.as_deref(), Some("Find the config"));
    assert_eq!(sub.model.as_deref(), Some("anthropic/sonnet"));

    let parallel = json!({"tasks": [{"agent": "reviewer", "task": "Review"}, {"agent": "scout", "task": "Look"}]});
    let sub = Subagent::of_call(None, PI_SPAWN, Some(&parallel)).expect("parallel subagents");
    assert_eq!(sub.name.as_deref(), Some("reviewer"));

    let chain = json!({"chain": [{"agent": "planner", "task": "Plan"}]});
    assert!(Subagent::of_call(None, PI_SPAWN, Some(&chain)).is_some());

    let status = json!({"action": "status", "agent": "scout"});
    assert!(Subagent::of_call(None, PI_SPAWN, Some(&status)).is_none());
}
