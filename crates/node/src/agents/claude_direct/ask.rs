//! What the CLI's `can_use_tool` requests become for the client, and how its
//! answers go back: a permission request for a tool, plan approval for
//! `ExitPlanMode`, and a form for `AskUserQuestion`'s questions, in the
//! schema Claude Code's adapter uses.

use crate::agents::acp::bridge::{choice_options, selected_option};
use agent_client_protocol::v1::schema::{
    CreateElicitationRequest, CreateElicitationResponse, ElicitationAction, PermissionOption,
    PermissionOptionKind, RequestPermissionResponse, SessionId,
};
use serde_json::{Map, Value, json};

const ALLOW: &str = "allow-once";
const ALWAYS: &str = "allow-with-updates";
const REJECT: &str = "reject";
/// The node recognizes plan approval by these ids.
const PLAN_BYPASS: &str = "exit-plan-bypass";
const PLAN_ACCEPT_EDITS: &str = "exit-plan-accept-edits";
const PLAN_DEFAULT: &str = "exit-plan-default";

/// What the CLI told the model when the user refused.
const REFUSED: &str = "The user refused permission to run this tool.";
const KEEP_PLANNING: &str = "The user chose to keep planning.";
const DISMISSED: &str = "The user dismissed the questions without answering.";

/// The choices for a tool call.
pub(super) fn tool_options(suggested: bool) -> Vec<PermissionOption> {
    let mut options = vec![PermissionOption::new(
        ALLOW,
        "Yes",
        PermissionOptionKind::AllowOnce,
    )];
    if suggested {
        options.push(PermissionOption::new(
            ALWAYS,
            "Yes, and don't ask again",
            PermissionOptionKind::AllowAlways,
        ));
    }
    options.push(PermissionOption::new(
        REJECT,
        "No",
        PermissionOptionKind::RejectOnce,
    ));
    options
}

/// What the CLI is told of the answer to a tool call's permission request:
/// allowed with `input`, keeping the rules it `suggested` for always, or
/// refused.
pub(super) fn tool_decision(
    answer: Result<RequestPermissionResponse, agent_client_protocol::Error>,
    input: Value,
    suggested: Option<Value>,
) -> Value {
    match selected_option(answer).as_deref() {
        Some(ALLOW) => allow(input, None),
        Some(ALWAYS) => allow(input, suggested),
        _ => deny(REFUSED),
    }
}

/// The choices for a plan, made in `before` (the mode the session was in
/// before plan mode): back to bypassing permissions first when it came from
/// there.
pub(super) fn plan_options(before: &str) -> Vec<PermissionOption> {
    let mut options = Vec::new();
    if before == "bypassPermissions" {
        options.push(PermissionOption::new(
            PLAN_BYPASS,
            "Yes, and bypass permissions",
            PermissionOptionKind::AllowAlways,
        ));
    }
    options.extend([
        PermissionOption::new(
            PLAN_ACCEPT_EDITS,
            "Yes, and auto-accept edits",
            PermissionOptionKind::AllowAlways,
        ),
        PermissionOption::new(
            PLAN_DEFAULT,
            "Yes, and manually approve edits",
            PermissionOptionKind::AllowOnce,
        ),
        PermissionOption::new(
            REJECT,
            "No, keep planning",
            PermissionOptionKind::RejectOnce,
        ),
    ]);
    options
}

/// What the CLI is told of a plan's approval, and the mode the session goes
/// on in when it was approved.
pub(super) fn plan_decision(
    answer: Result<RequestPermissionResponse, agent_client_protocol::Error>,
    input: Value,
) -> (Value, Option<&'static str>) {
    let mode = match selected_option(answer).as_deref() {
        Some(PLAN_BYPASS) => "bypassPermissions",
        Some(PLAN_ACCEPT_EDITS) => "acceptEdits",
        Some(PLAN_DEFAULT) => "default",
        _ => return (deny(KEEP_PLANNING), None),
    };
    let set = json!([{"type": "setMode", "mode": mode, "destination": "session"}]);
    (allow(input, Some(set)), Some(mode))
}

/// One question `AskUserQuestion` asks.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Question {
    text: String,
    multiple: bool,
}

/// The form that puts `input`'s questions to the user, and the questions;
/// `None` when it asks none that can be answered.
pub(super) fn form(
    session: &SessionId,
    tool_call: &str,
    input: &Value,
) -> Option<(CreateElicitationRequest, Vec<Question>)> {
    let asked: Vec<&Value> = input["questions"]
        .as_array()?
        .iter()
        .filter(|question| {
            question["question"].is_string()
                && question["options"]
                    .as_array()
                    .is_some_and(|options| !options.is_empty())
        })
        .collect();
    let single = asked.len() == 1;
    let mut properties = Map::new();

    for (index, question) in asked.iter().enumerate() {
        let choices = choice_options(question["options"].as_array()?);
        let multiple = question["multiSelect"] == true;
        let mut field = match multiple {
            true => json!({"type": "array", "items": {"anyOf": choices}}),
            false => json!({"type": "string", "oneOf": choices}),
        };
        if let Some(header) = question["header"].as_str() {
            field["title"] = json!(header);
        }
        if !single {
            field["description"] = question["question"].clone();
        }
        let custom = match multiple {
            true => "Type your own answer to add to your selection above (optional).",
            false => {
                "Type your own answer, or add a note to the option you chose above (optional)."
            }
        };
        properties.insert(format!("question_{index}"), field);
        properties.insert(
            format!("question_{index}_custom"),
            json!({"type": "string", "title": "Other", "description": custom}),
        );
    }

    let first = asked.first()?;
    let message = match single {
        true => first["question"].clone(),
        false => json!("Please answer the following questions."),
    };
    let request = json!({
        "mode": "form",
        "sessionId": session,
        "toolCallId": tool_call,
        "message": message,
        "requestedSchema": {"type": "object", "properties": properties},
    });
    let request = match serde_json::from_value(request) {
        Ok(request) => request,
        Err(e) => {
            log::warn!("AskUserQuestion's questions do not make a form: {e}");
            return None;
        }
    };
    let questions = asked
        .iter()
        .map(|question| Question {
            text: question["question"].as_str().unwrap_or_default().to_owned(),
            multiple: question["multiSelect"] == true,
        })
        .collect();
    Some((request, questions))
}

/// What the CLI is told of the answer to the form for `questions`: the tool's
/// `input` with `answers` by question text, a multiple choice joined as the
/// CLI joins it, and a note typed beside a single choice as its `notes`. A
/// declined form answers nothing; a cancelled one refuses the tool.
pub(super) fn answered(
    answer: Result<CreateElicitationResponse, agent_client_protocol::Error>,
    mut input: Value,
    questions: &[Question],
) -> Value {
    let content = match answer.map(|answer| answer.action) {
        Ok(ElicitationAction::Accept(accepted)) => {
            serde_json::to_value(accepted.content).unwrap_or_default()
        }
        Ok(ElicitationAction::Decline) => Value::Null,
        _ => return deny(DISMISSED),
    };
    let mut answers = Map::new();
    let mut notes = Map::new();

    for (index, question) in questions.iter().enumerate() {
        let custom = content[format!("question_{index}_custom")]
            .as_str()
            .map(str::trim)
            .filter(|text| !text.is_empty());
        let picked = &content[format!("question_{index}")];
        let mut picks: Vec<String> = match picked {
            Value::Array(items) => items.iter().filter_map(scalar).collect(),
            other => scalar(other).into_iter().collect(),
        };
        picks.retain(|pick| !pick.is_empty());

        if question.multiple {
            picks.extend(custom.map(str::to_owned));
            if !picks.is_empty() {
                answers.insert(question.text.clone(), json!(joined(&picks)));
            }
            continue;
        }

        match (picks.is_empty(), custom) {
            (true, Some(custom)) => {
                answers.insert(question.text.clone(), json!(custom));
            }
            (true, None) => {}
            (false, custom) => {
                answers.insert(question.text.clone(), json!(picks.join(", ")));
                if let Some(custom) = custom {
                    notes.insert(question.text.clone(), json!({"notes": custom}));
                }
            }
        }
    }

    input["answers"] = Value::Object(answers);
    if !notes.is_empty() {
        input["annotations"] = Value::Object(notes);
    }
    allow(input, None)
}

/// A multiple choice as the CLI's own question UI writes it: joined with
/// `, `, an item that holds that or a quote quoted as JSON.
fn joined(items: &[String]) -> String {
    items
        .iter()
        .map(|item| match item.contains(", ") || item.contains('"') {
            true => Value::String(item.clone()).to_string(),
            false => item.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn scalar(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

fn allow(input: Value, permissions: Option<Value>) -> Value {
    let mut decision = json!({"behavior": "allow", "updatedInput": input});
    if let Some(permissions) = permissions.filter(|permissions| !permissions.is_null()) {
        decision["updatedPermissions"] = permissions;
    }
    decision
}

pub(super) fn deny(message: &str) -> Value {
    json!({"behavior": "deny", "message": message})
}
