//! The actions a turn took, as one line each for memory and the actions log.

use super::{
    DENIED, SKIPPED, ToolApp, ToolSlug, action_name, fields, guard::READ_ONLY, lookup,
    parse_output, records, shorten,
};
use crate::protocol::ApprovalField;
use chrono::{DateTime, Utc};
use flashmind::{
    core::{Conversation, EntryKind},
    types::ToolCall,
};
use serde_json::Value;
use std::collections::HashMap;

/// Longest field value an action's description keeps.
pub(super) const MAX_ACTION_VALUE_CHARS: usize = 80;

/// Fields an action's description keeps.
pub(super) const MAX_ACTION_FIELDS: usize = 4;

/// One line per acting call that went through since `since`, such as "Gmail: send email (Subject: Lunch)".
pub(crate) fn actions_since(conversation: &Conversation, since: DateTime<Utc>) -> Vec<String> {
    let entries = conversation.entries();
    let calls: HashMap<&str, &ToolCall> = entries
        .iter()
        .filter_map(|entry| match &entry.kind {
            EntryKind::Assistant {
                tool_calls: Some(calls),
                ..
            } => Some(calls),
            _ => None,
        })
        .flatten()
        .map(|call| (call.id.as_str(), call))
        .collect();
    entries
        .iter()
        .filter(|entry| entry.timestamp >= since)
        .filter_map(|entry| match &entry.kind {
            EntryKind::Tool {
                call_id, output, ..
            } => Some((calls.get(call_id.as_str())?, output)),
            _ => None,
        })
        .filter(|(_, output)| went_through(output))
        .filter_map(|(call, _)| {
            let (app, acts) = lookup(&call.name)?;
            acts.on(&call.arguments)
                .then(|| describe_action(&app, call, conversation))
        })
        .collect()
}

/// Whether a tool result says the call did what it was asked.
pub(crate) fn went_through(output: &str) -> bool {
    if output == DENIED || output == SKIPPED || output.starts_with(READ_ONLY) {
        return false;
    }
    let failed = parse_output(output).is_some_and(|result| {
        result.get("successful") == Some(&Value::Bool(false))
            || result.get("error").is_some_and(|error| !error.is_null())
    });
    !failed
}

pub(super) fn describe_action(
    app: &ToolApp,
    call: &ToolCall,
    conversation: &Conversation,
) -> String {
    let action = action_name(app, ToolSlug::new(&call.name).as_str());
    let mut fields = records(call, conversation);
    fields.extend(fields_of(&call.arguments));
    let details: Vec<String> = fields
        .into_iter()
        .take(MAX_ACTION_FIELDS)
        .map(|field| {
            let value = field.value.split_whitespace().collect::<Vec<_>>().join(" ");
            format!(
                "{}: {}",
                field.label,
                shorten(&value, MAX_ACTION_VALUE_CHARS)
            )
        })
        .collect();
    if details.is_empty() {
        format!("{}: {action}", app.name())
    } else {
        format!("{}: {action} ({})", app.name(), details.join("; "))
    }
}

/// The argument fields that say most about an action, before long text.
pub(super) fn fields_of(args: &Value) -> Vec<ApprovalField> {
    let mut fields = fields(args);
    fields.sort_by_key(|field| field.value.chars().count() > MAX_ACTION_VALUE_CHARS);
    fields
}
