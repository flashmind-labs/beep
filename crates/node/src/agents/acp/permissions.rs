//! Permission choices and plan approval requests.

use agent_client_protocol::schema::v1::{
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

#[cfg(test)]
#[path = "../../tests/agents_acp_permissions.rs"]
mod tests;
