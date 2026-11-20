use super::ask::{Asked, answer, to_client};
use super::events::{finished, update};
use crate::agents::acp::allow_option;
use beep_proto::node::agent::{AnswerOption, OptionKind};
use serde_json::{Value, json};

fn approval(allow_session: bool, allow_permanent: bool) -> Value {
    json!({"session_id": "12b450c6", "request_id": "ap1", "command": "rm -rf build", "description": "Delete the build",
        "choices": ["once", "session", "always", "deny"], "allow_permanent": allow_permanent, "allow_session": allow_session,
        "smart_denied": null, "tool_name": "terminal", "gateway_session_id": null})
}

fn selected(option: &str) -> Value {
    json!({"outcome": {"outcome": "selected", "optionId": option}})
}

#[test]
fn a_clarify_becomes_a_form_and_its_answer_goes_back_by_qid() {
    let params = json!({"session_id": "a3026a4a", "questions": [
        {"qid": "q0", "question": "Which color do you prefer?", "choices": ["Red (Recommended)", "Green", "Blue"], "multi_select": false},
        {"qid": "q1", "question": "Which sizes?", "choices": ["S", "M"], "multi_select": true}]});
    let (asked, form) = to_client("clarify", &params).unwrap();
    assert_eq!(form["mode"], "form");
    assert_eq!(form["requestedSchema"]["properties"]["q0"]["oneOf"][2]["const"], "Blue");
    assert_eq!(form["requestedSchema"]["properties"]["q1"]["type"], "array");

    let result = answer(&asked, &json!({"action": "accept", "content": {"q0": "Blue", "q1": ["S", "M"]}}));
    assert_eq!(result, json!({"answers": {"q0": "Blue", "q1": ["S", "M"]}}));
}

#[test]
fn an_approval_offers_always_only_when_hermes_allows_it() {
    let ids = |session, permanent| -> Vec<String> {
        let (_, request) = to_client("approval.request", &approval(session, permanent)).unwrap();
        request["options"].as_array().unwrap().iter().map(|o| o["optionId"].as_str().unwrap().to_owned()).collect()
    };

    assert_eq!(ids(false, false), ["once", "deny"]);
    assert_eq!(ids(true, false), ["once", "session", "deny"]);
    assert_eq!(ids(true, true), ["once", "session", "always", "deny"]);

    // An answer for an option Hermes did not offer is a refusal.
    let (asked, _) = to_client("approval.request", &approval(false, false)).unwrap();
    assert_eq!(answer(&asked, &selected("always")), json!({"choice": "deny"}));
}

#[test]
fn an_auto_allowed_approval_answers_once() {
    let (asked, request) = to_client("approval.request", &approval(true, true)).unwrap();
    let options: Vec<AnswerOption> = request["options"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| AnswerOption {
            id: o["optionId"].as_str().unwrap().to_owned(),
            name: o["name"].as_str().unwrap().to_owned(),
            kind: o["kind"].as_str().unwrap().parse().unwrap_or(OptionKind::Other),
        })
        .collect();

    let picked = allow_option(&options).unwrap();
    assert_eq!(answer(&asked, &selected(&picked.id)), json!({"choice": "once"}));
    assert_eq!(answer(&asked, &selected("session")), json!({"choice": "session"}));
    assert_eq!(answer(&asked, &selected("always")), json!({"choice": "always"}));
}

#[test]
fn a_declined_form_or_rejected_approval_denies() {
    let (asked, _) = to_client("approval.request", &approval(true, true)).unwrap();
    assert_eq!(answer(&asked, &selected("deny")), json!({"choice": "deny"}));
    assert_eq!(answer(&asked, &json!({"outcome": {"outcome": "cancelled"}})), json!({"choice": "deny"}));

    let asked = Asked::Clarify { qids: vec!["q0".into()] };
    let cancelled = json!({"answers": null, "outcome": "cancelled"});
    assert_eq!(answer(&asked, &json!({"action": "decline"})), cancelled);
    assert_eq!(answer(&asked, &json!({"action": "cancel"})), cancelled);
}

#[test]
fn message_and_thinking_deltas_become_chunks() {
    let message = json!({"type": "message.delta", "session_id": "12b450c6", "payload": {"text": " command", "turn_id": "t", "segment": 0, "offset": 5}, "seq": 15});
    assert_eq!(update(&message), vec![json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": " command"}})]);

    let reasoning = json!({"type": "reasoning.delta", "session_id": "12b450c6", "payload": {"text": "Let me think"}, "seq": 4});
    assert_eq!(update(&reasoning), vec![json!({"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": "Let me think"}})]);

    // The spinner text is not reasoning.
    let spinner = json!({"type": "thinking.delta", "session_id": "12b450c6", "payload": {"text": "( \u{2d8}\u{2323}\u{2d8})\u{2661} brainstorming..."}, "seq": 4});
    assert!(update(&spinner).is_empty());
    assert!(update(&json!({"type": "sessions.changed", "session_id": "", "payload": {}})).is_empty());
}

#[test]
fn a_tool_runs_from_start_to_complete_and_failures_show() {
    let start = json!({"type": "tool.start", "session_id": "s", "payload": {"tool_id": "call_60ba", "name": "terminal", "context": "echo hello", "args": {"command": "echo hello", "timeout": 30}}, "seq": 9});
    let started = &update(&start)[0];
    assert_eq!(started["sessionUpdate"], "tool_call");
    assert_eq!(started["toolCallId"], "call_60ba");
    assert_eq!(started["status"], "in_progress");
    assert_eq!(started["title"], "terminal");

    let done = json!({"type": "tool.complete", "session_id": "s", "payload": {"tool_id": "call_60ba", "name": "terminal", "args": {"command": "echo hello"}, "duration_s": 0.06, "result": {"output": "hello", "exit_code": 0, "error": null}}, "seq": 10});
    let ended = &update(&done)[0];
    assert_eq!(ended["sessionUpdate"], "tool_call_update");
    assert_eq!(ended["status"], "completed");
    assert_eq!(ended["content"][0]["content"]["text"], "hello");

    let bad_exit = json!({"type": "tool.complete", "payload": {"tool_id": "c", "name": "terminal", "result": {"output": "", "exit_code": 2, "error": null}}});
    assert_eq!(update(&bad_exit)[0]["status"], "failed");
    let errored = json!({"type": "tool.complete", "payload": {"tool_id": "c", "name": "terminal", "result": {"output": null, "exit_code": 0, "error": "boom"}}});
    assert_eq!(update(&errored)[0]["status"], "failed");
}

#[test]
fn a_title_and_the_end_of_a_turn() {
    let title = json!({"type": "session.title", "session_id": "s", "payload": {"session_id": "2026", "title": "Run echo hello in terminal"}, "seq": 5});
    assert_eq!(update(&title), vec![json!({"sessionUpdate": "session_info_update", "title": "Run echo hello in terminal"})]);

    assert!(finished(&json!({"type": "message.complete", "payload": {"text": "done"}})));
    assert!(!finished(&json!({"type": "message.delta", "payload": {"text": "x"}})));
}
