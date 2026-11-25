//! What Claude Code's frames show as ACP session updates: its streamed text
//! and thinking, its tool calls and their results, the context it used. A
//! transcript line has the shape of a stream frame, so a loaded session
//! replays its transcript through the same mapping.

use agent_client_protocol::v1::schema::{
    ContentBlock, ContentChunk, Cost, Diff, Meta, MessageId, SessionUpdate, StopReason, TextContent, ToolCall,
    ToolCallContent, ToolCallId, ToolCallLocation, ToolCallStatus, ToolCallUpdate,
    ToolCallUpdateFields, ToolKind, Usage, UsageUpdate,
};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// The text or thinking a `stream_event` frame streams of the main agent's
/// message `message`; a subagent's stays in its own tool call.
pub(super) fn delta(frame: &Value, message: Option<&str>) -> Option<SessionUpdate> {
    if !frame["parent_tool_use_id"].is_null() {
        return None;
    }

    let delta = &frame["event"]["delta"];
    let streamed = |key: &str| delta[key].as_str().filter(|text| !text.is_empty());
    match (frame["event"]["type"].as_str(), delta["type"].as_str()) {
        (Some("content_block_delta"), Some("text_delta")) => {
            Some(SessionUpdate::AgentMessageChunk(
                chunk(streamed("text")?).message_id(message.map(MessageId::new)),
            ))
        }
        (Some("content_block_delta"), Some("thinking_delta")) => Some(
            SessionUpdate::AgentThoughtChunk(chunk(streamed("thinking")?)),
        ),
        _ => None,
    }
}

/// What an `assistant` or `user` line shows: its tool calls and their
/// results, and with `replay` (a line of the transcript) its text, which the
/// live stream sends as deltas.
pub(super) fn message(line: &Value, replay: bool) -> Vec<SessionUpdate> {
    let parent = line["parent_tool_use_id"].as_str();
    let content = &line["message"]["content"];
    let blocks = content.as_array().map(Vec::as_slice).unwrap_or_default();
    let main = parent.is_none() && line["isSidechain"] != true;
    let id = line["message"]["id"].as_str().map(MessageId::new);

    match line["type"].as_str() {
        Some("assistant") => blocks
            .iter()
            .filter_map(|block| match block["type"].as_str()? {
                "text" if replay && main => text(block["text"].as_str()?)
                    .map(|text| SessionUpdate::AgentMessageChunk(chunk(text).message_id(id.clone()))),
                "thinking" if replay && main => text(block["thinking"].as_str()?)
                    .map(|text| SessionUpdate::AgentThoughtChunk(chunk(text))),
                "tool_use" => Some(SessionUpdate::ToolCall(tool_call(
                    block["id"].as_str()?,
                    block["name"].as_str()?,
                    &block["input"],
                    parent,
                ))),
                _ => None,
            })
            .collect(),
        Some("user") => {
            let mut updates: Vec<SessionUpdate> = blocks
                .iter()
                .filter(|block| block["type"] == "tool_result")
                .filter_map(tool_result)
                .collect();
            let said = replay
                && main
                && updates.is_empty()
                && (line["isMeta"] != true || line["isCompactSummary"] == true);

            if said && let Some(text) = text(&user_text(content)) {
                let mut replayed = chunk(text);
                if line["isCompactSummary"] == true {
                    replayed = replayed.meta(Meta::from_iter([(
                        "isCompactSummary".to_owned(),
                        json!(true),
                    )]));
                }
                updates.push(SessionUpdate::UserMessageChunk(replayed));
            }

            updates
        }
        _ => Vec::new(),
    }
}

/// The updates a session's transcript replays, in order.
pub(super) fn replay(path: &Path, in_wsl: bool) -> std::io::Result<Vec<SessionUpdate>> {
    let text = std::fs::read_to_string(path)?;
    Ok(text
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .map(|mut line| {
            if in_wsl {
                crate::machine::wsl::translate(&mut line, crate::machine::wsl::Toward::Windows);
            }
            line
        })
        .flat_map(|line| message(&line, true))
        .collect())
}

/// Where Claude Code keeps the transcript of `session`, run in `cwd`.
pub(super) fn transcript(cwd: &Path, session: &str) -> Option<PathBuf> {
    let folder = crate::agents::claude_dir::slug(cwd);
    Some(crate::agents::claude_dir::projects_dir()?.join(folder).join(format!("{session}.jsonl")))
}

/// The tool call `id` to the tool `name` with `input`, run inside the
/// subagent call `parent` when it has one.
pub(super) fn tool_call(id: &str, name: &str, input: &Value, parent: Option<&str>) -> ToolCall {
    let (title, kind) = describe(name, input);
    let path = ["file_path", "notebook_path", "path"]
        .iter()
        .find_map(|key| input[*key].as_str());
    let diff = match name {
        "Edit" => path.zip(input["new_string"].as_str()).map(|(path, new)| {
            Diff::new(path, new).old_text(input["old_string"].as_str().map(str::to_owned))
        }),
        "Write" => path
            .zip(input["content"].as_str())
            .map(|(path, new)| Diff::new(path, new)),
        _ => None,
    };
    ToolCall::new(id.to_owned(), title)
        .kind(kind)
        .status(ToolCallStatus::Pending)
        .raw_input(input.clone())
        .locations(path.map(ToolCallLocation::new).into_iter().collect())
        .content(diff.map(ToolCallContent::Diff).into_iter().collect())
        .meta(meta(name, parent))
}

/// The tool call update a permission request names: the call as the stream
/// showed it.
pub(super) fn asked(id: &str, name: &str, input: &Value) -> ToolCallUpdate {
    let (title, kind) = describe(name, input);
    ToolCallUpdate::new(
        id.to_owned(),
        ToolCallUpdateFields::new()
            .title(title)
            .kind(kind)
            .raw_input(input.clone()),
    )
}

/// The update of the call a `tool_result` block ends.
fn tool_result(block: &Value) -> Option<SessionUpdate> {
    let id = block["tool_use_id"].as_str()?;
    let status = match block["is_error"] == true {
        true => ToolCallStatus::Failed,
        false => ToolCallStatus::Completed,
    };
    let text = user_text(&block["content"]);
    let content = (!text.is_empty())
        .then(|| ToolCallContent::from(ContentBlock::Text(TextContent::new(text))));
    Some(SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
        ToolCallId::new(id.to_owned()),
        ToolCallUpdateFields::new()
            .status(status)
            .content(content.into_iter().collect::<Vec<_>>()),
    )))
}

/// A title and kind for a call to `name`, as Claude Code's adapter gives
/// them.
fn describe(name: &str, input: &Value) -> (String, ToolKind) {
    let field = |key: &str| input[key].as_str().filter(|value| !value.is_empty());
    let named = |verb: &str, key: &str| field(key).map(|value| format!("{verb} {value}"));
    let (title, kind) = match name {
        "Bash" => (field("command").map(str::to_owned), ToolKind::Execute),
        "Read" => (named("Read", "file_path"), ToolKind::Read),
        "Write" => (named("Write", "file_path"), ToolKind::Edit),
        "Edit" => (named("Edit", "file_path"), ToolKind::Edit),
        "NotebookEdit" => (named("Edit", "notebook_path"), ToolKind::Edit),
        "Glob" => (named("Find", "pattern"), ToolKind::Search),
        "Grep" => (named("grep", "pattern"), ToolKind::Search),
        "WebFetch" => (named("Fetch", "url"), ToolKind::Fetch),
        "WebSearch" => (
            field("query").map(|query| format!("Search \"{query}\"")),
            ToolKind::Fetch,
        ),
        "Task" | "Agent" => (field("description").map(str::to_owned), ToolKind::Think),
        "ExitPlanMode" => (Some("Approve Plan".to_owned()), ToolKind::SwitchMode),
        "EnterPlanMode" => (None, ToolKind::SwitchMode),
        "Skill" => (named("Load skill:", "skill"), ToolKind::Other),
        "AskUserQuestion" => (
            input["questions"][0]["question"].as_str().map(str::to_owned),
            ToolKind::Other,
        ),
        _ => (None, ToolKind::Other),
    };
    (title.unwrap_or_else(|| name.to_owned()), kind)
}

/// What the node reads of a call: the tool's name and the subagent call it
/// ran in, as Claude Code's adapter stamps them.
fn meta(name: &str, parent: Option<&str>) -> Meta {
    let mut claude = json!({"toolName": name});
    if let Some(parent) = parent {
        claude["parentToolUseId"] = json!(parent);
    }
    let mut meta = Meta::new();
    meta.insert("claudeCode".to_owned(), claude);
    meta
}

/// The text of message content: a string, or its text blocks joined.
fn user_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|block| block["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn text(text: &str) -> Option<&str> {
    (!text.trim().is_empty()).then_some(text)
}

fn chunk(text: &str) -> ContentChunk {
    ContentChunk::new(ContentBlock::Text(TextContent::new(text)))
}

/// How much of its window the main agent's context fills after
/// `assistant`, an `assistant` frame of its own, when it says.
pub(super) fn context(assistant: &Value) -> Option<u64> {
    if !assistant["parent_tool_use_id"].is_null() {
        return None;
    }

    let usage = &assistant["message"]["usage"];
    let tokens = [
        "input_tokens",
        "cache_read_input_tokens",
        "cache_creation_input_tokens",
        "output_tokens",
    ];
    tokens
        .iter()
        .map(|key| usage[*key].as_u64())
        .try_fold(0u64, |sum, count| Some(sum.saturating_add(count.unwrap_or(0))))
        .filter(|used| *used > 0)
}

/// The usage a `result` frame reports, with `used` of the context: the
/// session's cost so far and its window.
pub(super) fn usage_update(result: &Value, used: u64) -> Option<UsageUpdate> {
    let size = result["modelUsage"]
        .as_object()?
        .values()
        .filter_map(|model| model["contextWindow"].as_u64())
        .max()?;
    let cost = result["total_cost_usd"]
        .as_f64()
        .map(|amount| Cost::new(amount, "USD"));
    Some(UsageUpdate::new(used, size).cost(cost))
}

/// The tokens the turn a `result` frame ends used.
pub(super) fn turn_usage(result: &Value) -> Option<Usage> {
    let usage = &result["usage"];
    let count = |key: &str| usage[key].as_u64().unwrap_or(0);
    let input = count("input_tokens");
    let output = count("output_tokens");
    let read = count("cache_read_input_tokens");
    let wrote = count("cache_creation_input_tokens");
    let total = input + output + read + wrote;
    (total > 0).then(|| {
        Usage::new(total, input, output)
            .cached_read_tokens(read)
            .cached_write_tokens(wrote)
    })
}

/// Results that end work the model did on its own, such as after a
/// background task, not the user's prompt.
const AUTONOMOUS: &[&str] = &[
    "task-notification",
    "peer",
    "coordinator",
    "observer",
    "observer-activity",
];

/// Whether a `result` frame ends the user's prompt.
pub(super) fn ends_prompt(result: &Value) -> bool {
    !result["origin"]["kind"]
        .as_str()
        .is_some_and(|kind| AUTONOMOUS.contains(&kind))
}

/// How a `result` frame ends the prompt: a stop reason, or the error it
/// failed with. `cancelled` when the client cancelled it.
pub(super) fn stop(result: &Value, cancelled: bool) -> Result<StopReason, String> {
    let stop_reason = result["stop_reason"].as_str();
    let error = result["is_error"] == true;
    let aborted = result["terminal_reason"]
        .as_str()
        .is_some_and(|reason| reason.starts_with("aborted"));

    if stop_reason == Some("refusal") {
        return Ok(StopReason::Refusal);
    }

    if cancelled || aborted {
        return Ok(StopReason::Cancelled);
    }

    let failed = || {
        let errors: Vec<&str> = result["errors"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        let message = result["result"]
            .as_str()
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
            .or_else(|| (!errors.is_empty()).then(|| errors.join(", ")));
        Err(message.unwrap_or_else(|| result["subtype"].as_str().unwrap_or("failed").to_owned()))
    };

    match result["subtype"].as_str() {
        Some("success" | "error_during_execution") if stop_reason == Some("max_tokens") => {
            Ok(StopReason::MaxTokens)
        }
        Some("success" | "error_during_execution") if error => failed(),
        Some("success" | "error_during_execution") => Ok(StopReason::EndTurn),
        _ if error => failed(),
        _ => Ok(StopReason::MaxTurnRequests),
    }
}
