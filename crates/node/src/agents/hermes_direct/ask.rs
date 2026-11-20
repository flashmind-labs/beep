//! What Hermes's requests become for the client, and how its answers go
//! back: a permission request for an approval, and a form for the questions
//! of a clarify. The bridge adds `sessionId` to what is built here.

use serde_json::{Map, Value, json};

/// What a request put to the client asked, to read its answer by.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Asked {
    Approval {
        request_id: String,
        allow_session: bool,
        allow_permanent: bool,
    },
    Clarify {
        qids: Vec<String>,
    },
}

/// The ACP request for Hermes's `method` request with `params`, and what it
/// asks. `None` for a request the client is not asked.
pub(super) fn to_client(method: &str, params: &Value) -> Option<(Asked, Value)> {
    match method {
        "approval.request" | "approval" => approval(params),
        "clarify" => clarify(params),
        _ => None,
    }
}

/// The JSON-RPC `result` for Hermes from the client's `reply` to what it
/// `asked`: a permission outcome or a form action.
pub(super) fn answer(asked: &Asked, reply: &Value) -> Value {
    match asked {
        Asked::Approval { allow_session, allow_permanent, .. } => {
            let outcome = &reply["outcome"];
            let picked = (outcome["outcome"] == "selected").then(|| outcome["optionId"].as_str()).flatten();
            let choice = match picked {
                Some("once") => "once",
                Some("session") if *allow_session => "session",
                Some("always") if *allow_permanent => "always",
                _ => "deny",
            };
            json!({"choice": choice})
        }
        Asked::Clarify { qids } => {
            if reply["action"] != "accept" {
                return json!({"answers": null, "outcome": "cancelled"});
            }
            let answers: Map<String, Value> = qids
                .iter()
                .filter_map(|qid| Some((qid.clone(), reply["content"].get(qid)?.clone())))
                .collect();
            json!({"answers": answers})
        }
    }
}

/// A permission request. Every allow is of the kind `allow_once` and `once`
/// comes first, so an auto-allow, which prefers `allow_always`, never finds
/// anything but `once`: only the user picks the session or always.
fn approval(params: &Value) -> Option<(Asked, Value)> {
    let request_id = params["request_id"].as_str()?.to_owned();
    let allow_session = params["allow_session"] == true;
    let allow_permanent = params["allow_permanent"] == true;
    let command = params["command"].as_str().filter(|command| !command.is_empty());
    let description = params["description"].as_str().filter(|text| !text.is_empty());

    let mut options = vec![json!({"optionId": "once", "name": "Yes, just this once", "kind": "allow_once"})];
    if allow_session {
        options.push(json!({"optionId": "session", "name": "Yes, for this session", "kind": "allow_once"}));
    }
    if allow_permanent {
        options.push(json!({"optionId": "always", "name": "Yes, always", "kind": "allow_once"}));
    }
    options.push(json!({"optionId": "deny", "name": "No", "kind": "reject_once"}));

    let content: Vec<Value> = description
        .into_iter()
        .map(|text| json!({"type": "content", "content": {"type": "text", "text": text}}))
        .collect();
    let request = json!({
        "toolCall": {
            "toolCallId": request_id,
            "title": command.or(description).unwrap_or("Run command"),
            "kind": "execute",
            "status": "pending",
            "rawInput": {"command": params["command"], "tool": params["tool_name"]},
            "content": content,
        },
        "options": options,
    });

    Some((Asked::Approval { request_id, allow_session, allow_permanent }, request))
}

/// A form with a field per question: a choice for one with choices (many
/// for a multi-select), else text.
fn clarify(params: &Value) -> Option<(Asked, Value)> {
    let questions = params["questions"].as_array()?;
    let mut properties = Map::new();
    let mut qids = Vec::new();

    for question in questions {
        let Some(qid) = question["qid"].as_str() else {
            continue;
        };
        let title = question["question"].as_str().filter(|text| !text.is_empty()).unwrap_or(qid);
        let choices: Vec<Value> = question["choices"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(|choice| json!({"const": choice, "title": choice}))
            .collect();
        let field = match (choices.is_empty(), question["multi_select"] == true) {
            (true, _) => json!({"type": "string", "title": title}),
            (false, false) => json!({"type": "string", "title": title, "oneOf": choices}),
            (false, true) => json!({"type": "array", "title": title, "items": {"anyOf": choices}}),
        };
        properties.insert(qid.to_owned(), field);
        qids.push(qid.to_owned());
    }

    if qids.is_empty() {
        return None;
    }

    let form = json!({
        "mode": "form",
        "message": "Hermes needs your input to continue.",
        "requestedSchema": {"type": "object", "properties": properties, "required": qids},
    });

    Some((Asked::Clarify { qids }, form))
}
