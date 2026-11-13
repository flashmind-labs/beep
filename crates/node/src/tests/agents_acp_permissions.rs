use super::*;

fn option(id: &str, kind: OptionKind) -> AnswerOption {
    AnswerOption {
        id: id.into(),
        name: id.into(),
        kind,
    }
}

fn allowed(options: &[AnswerOption]) -> Option<&str> {
    allow_option(options).map(|option| option.id.as_str())
}

#[test]
fn a_permission_request_is_allowed_for_good_when_it_can_be() {
    use OptionKind::*;
    // Codex, OpenCode and Gemini CLI: once, always, reject.
    let request = [
        option("approved", AllowOnce),
        option("approved-for-session", AllowAlways),
        option("abort", RejectOnce),
    ];
    assert_eq!(allowed(&request), Some("approved-for-session"));
    // Claude Code's prompt for a safety check has no "always".
    let request = [
        option("allow-once", AllowOnce),
        option("reject", RejectOnce),
    ];
    assert_eq!(allowed(&request), Some("allow-once"));
    // A request with no way to allow is still asked.
    let request = [option("reject", RejectOnce)];
    assert_eq!(allowed(&request), None);
}

#[test]
fn plan_approval_keeps_the_context() {
    use OptionKind::*;
    // Claude Code's ExitPlanMode after a session in bypassPermissions.
    let request = [
        option("exit-plan-clear-bypass", AllowAlways),
        option("exit-plan-bypass", AllowAlways),
        option("exit-plan-auto", AllowAlways),
        option("exit-plan-default", AllowOnce),
        option("reject", RejectOnce),
    ];
    assert_eq!(allowed(&request), Some("exit-plan-bypass"));
}

#[test]
fn a_choice_with_nothing_to_reject_is_left_to_the_user() {
    use OptionKind::*;
    // pi's extension select: every choice is an allow_once option.
    let choice = [option("choice-0", AllowOnce), option("choice-1", AllowOnce)];
    assert_eq!(allowed(&choice), None);
    assert_eq!(allowed(&[]), None);
}

#[test]
fn v2_permissions_keep_description_and_unknown_choice_kinds() {
    let permission = permission_v2(&serde_json::json!({
        "sessionId":"s", "title":"Choose", "description":"Pick an action",
        "options":[{"optionId":"custom", "name":"Custom", "kind":"_future_kind"}]
    }))
    .unwrap();
    assert_eq!(permission.question.text, "Choose");
    assert_eq!(
        permission.question.detail.as_deref(),
        Some("Pick an action")
    );
    assert_eq!(permission.question.options[0].kind, OptionKind::Other);
    assert!(allow_option(&permission.question.options).is_none());
}

#[test]
fn v2_permissions_recognize_plan_approval_subjects() {
    let permission = permission_v2(&serde_json::json!({
        "sessionId":"s", "title":"Approve plan",
        "subject":{"type":"tool_call", "toolCall":{"toolCallId":"plan-one", "rawInput":{"plan":"Read and test"}}},
        "options":[{"optionId":"exit-plan-allow", "name":"Allow", "kind":"allow_once"},
                   {"optionId":"reject", "name":"Reject", "kind":"reject_once"}]
    })).unwrap();
    assert_eq!(permission.plan.as_deref(), Some("Read and test"));
}
