//! What Codex's requests become for the client, and how its answers go
//! back: a permission request for a command, a file change or more
//! permissions, and a form for the questions Codex asks mid-turn or an MCP
//! server's elicitation, with the options and schemas Codex's adapter uses.

use super::items::strip_shell;
use crate::agents::acp::bridge::{ClientReply, ClientRequest, choice_options, selected_option};
use agent_client_protocol::v1::schema::{
    CreateElicitationRequest, CreateElicitationResponse, ElicitationAction,
    RequestPermissionRequest, SessionId,
};
use serde_json::{Map, Value, json};
use std::collections::HashSet;

/// The choice Codex's adapter adds for an answer of the user's own.
const OTHER: &str = "None of the above";
/// How a note typed beside a choice reaches Codex.
const NOTE: &str = "user_note: ";

/// What a request put to the client asked, to read its answer by.
pub(super) enum Asked {
    /// A command, with the decision each option stands for.
    Command(Vec<(String, Value)>),
    FileChange,
    Permissions(Value),
    Questions(Vec<Question>),
    Elicitation,
}

/// A question Codex asks mid-turn.
pub(super) struct Question {
    id: String,
    /// The field that takes a note beside a choice, when it has one.
    note: Option<String>,
}

/// What the client is asked for Codex's `method` request with `params`:
/// a permission request or a form, and what it asks. `None` for a request
/// the client is not asked, such as a form when it takes none.
pub(super) fn ask(
    session: &SessionId,
    method: &str,
    params: &Value,
    forms: bool,
) -> Option<(ClientRequest, Asked)> {
    match method {
        "item/commandExecution/requestApproval" => {
            let (options, decisions) = command_options(params);
            let call = command_call(params);
            Some((
                ClientRequest::Permission(permission(session, call, options)?),
                Asked::Command(decisions),
            ))
        }
        "item/fileChange/requestApproval" => {
            let call = json!({"toolCallId": params["itemId"], "title": "Edit files", "kind": "edit", "status": "pending"});
            let options = [
                ("allow_once", "Yes, proceed", "allow_once"),
                (
                    "allow_for_session",
                    "Yes, and don't ask again for these files",
                    "allow_always",
                ),
                (
                    "cancel",
                    "No, and tell Codex what to do differently",
                    "reject_once",
                ),
            ];
            Some((
                ClientRequest::Permission(permission(session, call, options.map(option).into())?),
                Asked::FileChange,
            ))
        }
        "item/permissions/requestApproval" => {
            let reason = params["reason"]
                .as_str()
                .filter(|reason| !reason.is_empty());
            let call = json!({
                "toolCallId": params["itemId"],
                "title": "Grant permissions?",
                "kind": "other",
                "status": "pending",
                "rawInput": params["permissions"],
                "content": reason.map(|reason| json!({"type": "content", "content": {"type": "text", "text": reason}})).into_iter().collect::<Vec<_>>(),
            });
            let options = [
                (
                    "allow_permissions_turn",
                    "Yes, grant these permissions for this turn",
                    "allow_once",
                ),
                (
                    "allow_permissions_turn_strict_auto_review",
                    "Yes, grant for this turn with strict auto review",
                    "allow_once",
                ),
                (
                    "allow_permissions_session",
                    "Yes, grant these permissions for this session",
                    "allow_always",
                ),
                (
                    "reject_permissions",
                    "No, continue without permissions",
                    "reject_once",
                ),
            ];
            let request = permission(session, call, options.map(option).into())?;
            Some((
                ClientRequest::Permission(request),
                Asked::Permissions(params["permissions"].clone()),
            ))
        }
        "item/tool/requestUserInput" if forms => {
            let (form, questions) = questions_form(session, params)?;
            Some((ClientRequest::Form(form), Asked::Questions(questions)))
        }
        "mcpServer/elicitation/request" if forms && params["mode"] == "form" => {
            let form = json!({
                "sessionId": session,
                "mode": "form",
                "message": params["message"],
                "requestedSchema": params["requestedSchema"],
            });
            let form = serde_json::from_value(form)
                .inspect_err(|e| log::warn!("an MCP server's form does not read: {e}"))
                .ok()?;
            Some((ClientRequest::Form(form), Asked::Elicitation))
        }
        _ => None,
    }
}

/// What Codex is told of the client's `reply` to what it `asked`.
pub(super) fn answer(asked: Asked, reply: ClientReply) -> Value {
    match (asked, reply) {
        (Asked::Command(decisions), ClientReply::Permission(reply)) => {
            let picked = selected_option(reply);
            let decision = decisions
                .into_iter()
                .find(|(id, _)| picked.as_deref() == Some(id.as_str()))
                .map_or_else(|| json!("cancel"), |(_, decision)| decision);
            json!({"decision": decision})
        }
        (Asked::FileChange, ClientReply::Permission(reply)) => {
            let decision = match selected_option(reply).as_deref() {
                Some("allow_once") => "accept",
                Some("allow_for_session") => "acceptForSession",
                _ => "cancel",
            };
            json!({"decision": decision})
        }
        (Asked::Permissions(permissions), ClientReply::Permission(reply)) => {
            let (scope, strict) = match selected_option(reply).as_deref() {
                Some("allow_permissions_turn") => ("turn", false),
                Some("allow_permissions_turn_strict_auto_review") => ("turn", true),
                Some("allow_permissions_session") => ("session", false),
                _ => return refused("item/permissions/requestApproval"),
            };
            let mut granted = Map::new();
            for key in ["network", "fileSystem"] {
                if !permissions[key].is_null() {
                    granted.insert(key.to_owned(), permissions[key].clone());
                }
            }
            json!({"permissions": granted, "scope": scope, "strictAutoReview": strict})
        }
        (Asked::Questions(questions), ClientReply::Form(reply)) => {
            json!({"answers": answers(reply, &questions)})
        }
        (Asked::Elicitation, ClientReply::Form(reply)) => match reply.map(|reply| reply.action) {
            Ok(ElicitationAction::Accept(accepted)) => {
                json!({"action": "accept", "content": accepted.content})
            }
            Ok(ElicitationAction::Decline) => json!({"action": "decline", "content": null}),
            _ => json!({"action": "cancel", "content": null}),
        },
        (_, _) => json!({"decision": "cancel"}),
    }
}

/// What Codex is told when the client cannot be asked `method`'s request;
/// null for a request that has no such answer, which is refused.
pub(super) fn refused(method: &str) -> Value {
    match method {
        "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
            json!({"decision": "decline"})
        }
        "item/permissions/requestApproval" => {
            json!({"permissions": {}, "scope": "turn", "strictAutoReview": false})
        }
        "item/tool/requestUserInput" => json!({"answers": {}}),
        "mcpServer/elicitation/request" => json!({"action": "decline", "content": null}),
        _ => Value::Null,
    }
}

/// The options for a command, and the decision each stands for: the
/// decisions Codex offers, or by default allow once or for the session, and
/// decline or cancel.
fn command_options(params: &Value) -> (Vec<Value>, Vec<(String, Value)>) {
    let network = !params["networkApprovalContext"].is_null();
    let offered = params["availableDecisions"]
        .as_array()
        .filter(|offered| !offered.is_empty())
        .cloned();
    let decisions = offered.unwrap_or_else(|| match params["additionalPermissions"].is_null() {
        _ if network => vec![json!("accept"), json!("acceptForSession"), json!("decline"), json!("cancel")],
        false => vec![json!("accept"), json!("cancel")],
        true => {
            let mut decisions = vec![json!("accept"), json!("acceptForSession")];
            if params["proposedExecpolicyAmendment"].as_array().is_some_and(|amendment| !amendment.is_empty()) {
                decisions.push(json!({"acceptWithExecpolicyAmendment": {"execpolicy_amendment": params["proposedExecpolicyAmendment"]}}));
            }
            decisions.extend([json!("decline"), json!("cancel")]);
            decisions
        }
    });

    let mut options: Vec<(Value, (String, Value))> = Vec::new();
    let mut ids = HashSet::new();
    for (index, decision) in decisions.into_iter().enumerate() {
        let (id, name, kind) = match decision.as_str() {
            Some("accept") if network => (
                "allow_once".to_owned(),
                "Yes, just this once".to_owned(),
                "allow_once",
            ),
            Some("accept") => (
                "allow_once".to_owned(),
                "Yes, proceed".to_owned(),
                "allow_once",
            ),
            Some("acceptForSession") => {
                let name = match (network, params["additionalPermissions"].is_null()) {
                    (true, _) => "Yes, and allow this host for this conversation",
                    (false, false) => "Yes, and allow these permissions for this session",
                    (false, true) => "Yes, and don't ask again for this command in this session",
                };
                (
                    "allow_for_session".to_owned(),
                    name.to_owned(),
                    "allow_always",
                )
            }
            Some("decline") => (
                "decline".to_owned(),
                "No, continue without running it".to_owned(),
                "reject_once",
            ),
            Some("cancel") => (
                "cancel".to_owned(),
                "No, and tell Codex what to do differently".to_owned(),
                "reject_once",
            ),
            _ => match (
                &decision["acceptWithExecpolicyAmendment"],
                &decision["applyNetworkPolicyAmendment"],
            ) {
                (amendment, _) if amendment.is_object() => {
                    let prefix: Vec<&str> = amendment["execpolicy_amendment"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .collect();
                    let prefix = prefix.join(" ");
                    if prefix.contains(['\n', '\r']) {
                        continue;
                    }
                    let name =
                        format!("Yes, and don't ask again for commands that start with `{prefix}`");
                    (
                        "accept_execpolicy_amendment".to_owned(),
                        name,
                        "allow_always",
                    )
                }
                (_, amendment) if amendment.is_object() => {
                    let allow = amendment["network_policy_amendment"]["action"] == "allow";
                    let (name, kind) = match allow {
                        true => ("Yes, and allow this host in the future", "allow_always"),
                        false => ("No, and block this host in the future", "reject_always"),
                    };
                    (
                        format!("apply_network_policy_amendment:{index}"),
                        name.to_owned(),
                        kind,
                    )
                }
                _ => continue,
            },
        };
        if ids.insert(id.clone()) {
            options.push((
                json!({"optionId": id, "name": name, "kind": kind}),
                (id, decision),
            ));
        }
    }

    // Allowing first, then allowing always, then refusing.
    options.sort_by_key(|(option, _)| match option["kind"].as_str() {
        Some("allow_once") => 0,
        Some("allow_always") => 1,
        _ => 2,
    });
    options.into_iter().unzip()
}

/// The tool call a command's approval names.
fn command_call(params: &Value) -> Value {
    let network = &params["networkApprovalContext"];
    let command = params["command"].as_str().map(strip_shell);
    let title = match (network.is_object(), &command) {
        (true, _) => format!(
            "{} network access to {}",
            text(network, "protocol"),
            text(network, "host")
        ),
        (false, Some(command)) => command.clone(),
        (false, None) => "Run command".to_owned(),
    };
    let mut input = Map::new();
    if let Some(command) = command {
        input.insert("command".to_owned(), json!(command));
    }
    if let Some(cwd) = params["cwd"].as_str() {
        input.insert("cwd".to_owned(), json!(cwd));
    }
    let lines: Vec<Value> = [
        network.is_object().then(|| {
            format!(
                "{} access to {}",
                text(network, "protocol"),
                text(network, "host")
            )
        }),
        params["reason"]
            .as_str()
            .filter(|reason| !reason.is_empty())
            .map(str::to_owned),
    ]
    .into_iter()
    .flatten()
    .map(|line| json!({"type": "content", "content": {"type": "text", "text": line}}))
    .collect();

    json!({
        "toolCallId": params["itemId"],
        "title": title,
        "kind": "execute",
        "status": "pending",
        "rawInput": input,
        "content": lines,
    })
}

fn permission(
    session: &SessionId,
    call: Value,
    options: Vec<Value>,
) -> Option<RequestPermissionRequest> {
    serde_json::from_value(json!({"sessionId": session, "toolCall": call, "options": options}))
        .inspect_err(|e| log::warn!("Codex's approval does not make a permission request: {e}"))
        .ok()
}

fn option((id, name, kind): (&str, &str, &str)) -> Value {
    json!({"optionId": id, "name": name, "kind": kind})
}

/// The form for the questions of a `requestUserInput`, as Codex's adapter
/// builds it: a choice for a question with options, with a note field when
/// it takes an answer of the user's own, else a text field.
fn questions_form(
    session: &SessionId,
    params: &Value,
) -> Option<(CreateElicitationRequest, Vec<Question>)> {
    let asked = params["questions"].as_array()?;
    let ids: HashSet<&str> = asked
        .iter()
        .filter_map(|question| question["id"].as_str())
        .collect();
    let mut properties = Map::new();
    let mut required = Vec::new();
    let mut questions = Vec::new();

    for question in asked {
        let Some(id) = question["id"].as_str() else {
            continue;
        };
        let options = question["options"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default();
        let other = question["isOther"] == true && !options.is_empty();
        let title = [&question["question"], &question["header"]]
            .into_iter()
            .find_map(|text| text.as_str().filter(|text| !text.is_empty()))
            .unwrap_or(id);
        let mut field = json!({
            "title": title,
            "type": "string",
            "_meta": {"codex": {"isOther": question["isOther"], "isSecret": question["isSecret"]}},
        });
        if let Some(header) = question["header"].as_str()
            && !header.is_empty()
        {
            field["description"] = json!(header);
        }

        if !options.is_empty() {
            let mut choices = choice_options(options);

            if other && !choices.iter().any(|choice| choice["const"] == OTHER) {
                choices.push(json!({"const": OTHER, "title": OTHER, "description": "Provide a different answer in the note field."}));
            }
            field["oneOf"] = json!(choices);
        }

        properties.insert(id.to_owned(), field);
        required.push(json!(id));
        let note = other.then(|| {
            let note = (0..)
                .map(|n| match n {
                    0 => format!("{id}_note"),
                    n => format!("{id}_note{n}"),
                })
                .find(|note| !ids.contains(note.as_str()))
                .unwrap_or_default();
            properties.insert(
                note.clone(),
                json!({
                    "type": "string",
                    "title": "Additional answer or note",
                    "_meta": {"codex": {"questionId": id, "role": "user_note", "isSecret": question["isSecret"]}},
                }),
            );
            note
        });
        questions.push(Question {
            id: id.to_owned(),
            note,
        });
    }

    if questions.is_empty() {
        return None;
    }

    let form = json!({
        "sessionId": session,
        "toolCallId": params["itemId"],
        "mode": "form",
        "message": "Codex needs your input to continue.",
        "requestedSchema": {"type": "object", "properties": properties, "required": required},
        "_meta": {"codex": {"autoResolutionMs": params["autoResolutionMs"]}},
    });
    let form = serde_json::from_value(form)
        .inspect_err(|e| log::warn!("Codex's questions do not make a form: {e}"))
        .ok()?;
    Some((form, questions))
}

/// Codex's answers from the form's `reply`: each question's choice or
/// words, with a note as `user_note: …`. A form not filled in answers none.
fn answers(
    reply: Result<CreateElicitationResponse, agent_client_protocol::Error>,
    questions: &[Question],
) -> Value {
    let Ok(ElicitationAction::Accept(accepted)) = reply.map(|reply| reply.action) else {
        return json!({});
    };
    let content = serde_json::to_value(accepted.content).unwrap_or_default();
    let values = |key: &str| -> Vec<String> {
        match &content[key] {
            Value::String(text) if !text.trim().is_empty() => vec![text.clone()],
            Value::Array(items) => items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect(),
            _ => Vec::new(),
        }
    };
    let mut answers = Map::new();

    for question in questions {
        let mut given = values(&question.id);
        if let Some(note) = &question.note {
            given.extend(
                values(note)
                    .into_iter()
                    .map(|note| format!("{NOTE}{}", note.trim())),
            );
        }
        if !given.is_empty() {
            answers.insert(question.id.clone(), json!({"answers": given}));
        }
    }

    Value::Object(answers)
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or_default()
}
