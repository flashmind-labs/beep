//! Permission choices and plan approval requests.

use agent_client_protocol::v1::schema::{
    PermissionOptionId, RequestPermissionOutcome, RequestPermissionRequest,
    RequestPermissionResponse, SelectedPermissionOutcome,
};
use beep_proto::node::agent::{AnswerOption, OptionKind};
use serde_json::Value;

/// The option that allows what a permission request asks, preferring one
/// that keeps allowing it. `None` when nothing in it rejects: then it is a
/// choice put to the user, such as a pi extension's select, not a request
/// for permission.
///
/// Claude Code's plan approval leads with options that also clear the
/// context (`exit-plan-clear-*`); those are passed over.
pub(in crate::agents) fn allow_option(options: &[AnswerOption]) -> Option<&AnswerOption> {
    if !options.iter().any(|option| option.kind.rejects()) {
        return None;
    }

    let keeps_context = |option: &&AnswerOption| !option.id.starts_with("exit-plan-clear");
    options
        .iter()
        .filter(keeps_context)
        .find(|option| option.kind == OptionKind::AllowAlways)
        .or_else(|| {
            options
                .iter()
                .filter(keeps_context)
                .find(|option| option.kind == OptionKind::AllowOnce)
        })
}

/// The plan a permission request asks to go ahead with, empty when it does
/// not say: Claude Code's ExitPlanMode, which carries it as `plan` in its input
/// and offers `exit-plan-*` options. `None` for any other request.
pub(in crate::agents) fn plan_of(request: &RequestPermissionRequest) -> Option<String> {
    let plan = request
        .tool_call
        .fields
        .raw_input
        .as_ref()
        .and_then(|input| input.get("plan"))
        .and_then(Value::as_str);
    let exits = request
        .options
        .iter()
        .any(|option| option.option_id.0.starts_with("exit-plan"));
    (plan.is_some() || exits).then(|| plan.unwrap_or_default().to_owned())
}

pub(in crate::agents) fn selected(option: &str) -> RequestPermissionResponse {
    RequestPermissionResponse::new(RequestPermissionOutcome::Selected(
        SelectedPermissionOutcome::new(PermissionOptionId::new(option)),
    ))
}

pub(in crate::agents) fn cancelled() -> RequestPermissionResponse {
    RequestPermissionResponse::new(RequestPermissionOutcome::Cancelled)
}

/// The question shown by either protocol, and an optional plan to approve.
pub(in crate::agents) struct Permission {
    pub question: beep_proto::node::agent::Question,
    pub plan: Option<String>,
}

/// V2 permission prompts have their own title and description. Unknown option
/// kinds remain choices instead of making the whole request unreadable.
pub(in crate::agents) fn permission_v2(
    params: &Value,
) -> Result<Permission, agent_client_protocol::Error> {
    use agent_client_protocol::{schema::MaybeUndefined, v2::schema as v2};
    use beep_proto::node::agent::{AnswerOption, Question};
    let request: v2::RequestPermissionRequest =
        serde_json::from_value(params.clone()).map_err(super::version::invalid)?;
    let mut plan = None;
    let subject_detail = match &request.subject {
        Some(v2::RequestPermissionSubject::ToolCall(subject)) => {
            if let MaybeUndefined::Value(input) = &subject.tool_call.raw_input {
                plan = input.get("plan").and_then(Value::as_str).map(str::to_owned);
                Some(input.to_string())
            } else {
                None
            }
        }
        Some(v2::RequestPermissionSubject::Command(command)) => Some(command.command.clone()),
        _ => None,
    };
    if plan.is_none()
        && request
            .options
            .iter()
            .any(|option| option.option_id.0.starts_with("exit-plan"))
    {
        plan = Some(String::new());
    }
    let options = request
        .options
        .into_iter()
        .map(|option| AnswerOption {
            id: option.option_id.to_string(),
            name: option.name,
            kind: match option.kind {
                v2::PermissionOptionKind::AllowOnce => OptionKind::AllowOnce,
                v2::PermissionOptionKind::AllowAlways => OptionKind::AllowAlways,
                v2::PermissionOptionKind::RejectOnce => OptionKind::RejectOnce,
                v2::PermissionOptionKind::RejectAlways => OptionKind::RejectAlways,
                _ => OptionKind::Other,
            },
        })
        .collect();
    Ok(Permission {
        question: Question {
            id: 0,
            text: crate::text::cut(
                &request.title,
                beep_proto::node::agent::MAX_QUESTION_LINE,
            ),
            detail: request.description.or(subject_detail).map(|detail| {
                crate::text::cut(&detail, beep_proto::node::agent::MAX_QUESTION_LINE)
            }),
            options,
        },
        plan,
    })
}

#[cfg(test)]
#[path = "../../tests/agents_acp_permissions.rs"]
mod tests;
