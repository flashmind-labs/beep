//! What Codex's thread items show as ACP session updates, as Codex's adapter
//! shows them: messages and reasoning as chunks; commands, file changes, MCP
//! and dynamic tool calls, web searches, images and subagents as tool calls.
//! A loaded session replays its turns' items through the same mapping.

use agent_client_protocol::v1::schema::SessionUpdate;
use regex::Regex;
use serde_json::{Value, json};
use std::sync::LazyLock;

/// The title Codex's adapter gives a file change.
const EDITING: &str = "Editing files";

/// The update `update` names, as JSON; `None`, logged, when it does not read.
pub(super) fn update(update: Value) -> Option<SessionUpdate> {
    serde_json::from_value(update)
        .inspect_err(|e| log::warn!("an update for Codex's item does not read: {e}"))
        .ok()
}

/// A text chunk of the kind `kind`, such as `agent_message_chunk`.
pub(super) fn chunk(kind: &str, text: &str) -> Value {
    json!({"sessionUpdate": kind, "content": {"type": "text", "text": text}})
}

/// The tool call an item that started shows; `None` for an item that is
/// no tool call.
pub(super) fn started(item: &Value) -> Option<Value> {
    let id = item["id"].as_str()?;
    let mut call = match item["type"].as_str()? {
        "commandExecution" => command(item),
        "fileChange" => json!({
            "title": EDITING,
            "kind": "edit",
            "content": item["changes"].as_array().into_iter().flatten().flat_map(diffs).collect::<Vec<_>>(),
        }),
        "mcpToolCall" => json!({
            "title": format!("mcp.{}.{}", text(item, "server"), text(item, "tool")),
            "kind": "execute",
            "rawInput": {"server": item["server"], "tool": item["tool"], "arguments": item["arguments"]},
            "_meta": {"is_mcp_tool_call": true},
        }),
        "dynamicToolCall" => json!({
            "title": format!("{}{}", text(item, "namespace"), text(item, "tool")),
            "kind": "other",
            "rawInput": item["arguments"],
        }),
        "webSearch" => json!({
            "title": web_search_title(item),
            "kind": "search",
            "rawInput": {"type": item["type"], "id": item["id"], "query": item["query"], "action": item["action"]},
        }),
        "imageView" => json!({
            "title": "View image",
            "kind": "read",
            "locations": [{"path": item["path"]}],
        }),
        "imageGeneration" => json!({"title": "Generate image", "kind": "other"}),
        "collabAgentToolCall" => {
            let prompt = item["prompt"].as_str().filter(|prompt| !prompt.is_empty());
            json!({
                "title": item["tool"],
                "kind": "other",
                "rawInput": collab_input(item),
                "content": prompt.map(text_content).into_iter().collect::<Vec<_>>(),
            })
        }
        "subAgentActivity" => {
            let name = item["agentPath"]
                .as_str()
                .and_then(|path| path.split('/').rfind(|part| !part.is_empty()))
                .unwrap_or("subagent");
            let verb = match item["kind"].as_str() {
                Some("interacted") => "Interact with",
                Some("interrupted") => "Interrupt",
                Some("completed") => "Complete",
                _ => "Start",
            };
            json!({
                "title": format!("{verb} subagent {name}"),
                "kind": "other",
                "rawInput": {"agentThreadId": item["agentThreadId"], "agentPath": item["agentPath"], "activityKind": item["kind"]},
            })
        }
        "contextCompaction" => json!({"title": "Compact conversation", "kind": "other"}),
        _ => return None,
    };

    call["sessionUpdate"] = json!("tool_call");
    call["toolCallId"] = json!(id);
    call["status"] = json!(match item["type"].as_str() {
        Some("webSearch" | "subAgentActivity" | "contextCompaction") => "in_progress",
        _ => status(&item["status"]),
    });
    Some(call)
}

/// The update an item that completed shows: the end of its tool call, or
/// the text of a message or reasoning when `whole` (it did not stream).
pub(super) fn completed(item: &Value, whole: bool) -> Option<Value> {
    let id = item["id"].as_str()?;
    let mut update = match item["type"].as_str()? {
        // A plan item is the plan Plan mode proposes, shown as its text.
        "agentMessage" | "plan" if whole => {
            return text_update("agent_message_chunk", item["text"].as_str()?);
        }
        "reasoning" if whole => {
            let parts = match item["summary"]
                .as_array()
                .is_some_and(|summary| !summary.is_empty())
            {
                true => &item["summary"],
                false => &item["content"],
            };
            let text: Vec<&str> = parts
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .filter(|part| !part.is_empty())
                .collect();
            return text_update("agent_thought_chunk", &text.join("\n\n"));
        }
        "commandExecution" => command_end(item),
        "mcpToolCall" => {
            let mut end = json!({"rawInput": {"server": item["server"], "tool": item["tool"], "arguments": item["arguments"]}});
            if !item["result"].is_null() || !item["error"].is_null() {
                end["rawOutput"] = json!({"result": item["result"], "error": item["error"]});
            }
            let shown = mcp_text(&item["result"])
                .or_else(|| item["error"]["message"].as_str().map(str::to_owned));
            end["content"] = json!(
                shown
                    .as_deref()
                    .map(text_content)
                    .into_iter()
                    .collect::<Vec<_>>()
            );
            end
        }
        "dynamicToolCall" => {
            let texts: Vec<Value> = item["contentItems"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|content| content["text"].as_str())
                .map(text_content)
                .collect();
            json!({"content": texts})
        }
        "collabAgentToolCall" => json!({"rawInput": collab_input(item)}),
        "subAgentActivity" => json!({
            "rawInput": {"agentThreadId": item["agentThreadId"], "agentPath": item["agentPath"], "activityKind": item["kind"]},
        }),
        "imageGeneration" => json!({
            "locations": item["savedPath"].as_str().map(|path| json!({"path": path})).into_iter().collect::<Vec<_>>(),
        }),
        "fileChange" | "webSearch" | "imageView" | "contextCompaction" => json!({}),
        _ => return None,
    };

    update["sessionUpdate"] = json!("tool_call_update");
    update["toolCallId"] = json!(id);
    update["status"] = json!(match item["type"].as_str() {
        Some("webSearch" | "subAgentActivity" | "contextCompaction" | "imageView") => "completed",
        _ if item["status"] == "completed" => "completed",
        _ => "failed",
    });
    Some(update)
}

/// The updates the item `item` of a turn in the history replays, `prompt`
/// being the text a user message shows.
pub(super) fn replayed(item: &Value) -> Vec<Value> {
    match item["type"].as_str() {
        Some("userMessage") => {
            let inputs = item["content"].as_array().into_iter().flatten();
            let said: Vec<&str> = inputs
                .clone()
                .filter(|input| input["type"] == "text")
                .filter_map(|input| input["text"].as_str())
                .collect();
            let images = inputs
                .filter(|input| input["type"] == "image")
                .filter_map(image);
            text_update("user_message_chunk", &said.join("\n"))
                .into_iter()
                .chain(images)
                .collect()
        }
        Some("agentMessage" | "reasoning" | "plan") => completed(item, true).into_iter().collect(),
        _ => started(item)
            .into_iter()
            .chain(completed(item, false))
            .collect(),
    }
}

/// The prompt's image `input` as Codex keeps it: a picture sent as a data
/// URL comes back whole, one from the web as its link.
fn image(input: &Value) -> Option<Value> {
    static DATA: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^data:([^;,]+);base64,(.*)$").expect("a valid pattern"));
    let url = input["url"].as_str()?;
    let content = match DATA.captures(url) {
        Some(data) => json!({"type": "image", "mimeType": &data[1], "data": &data[2]}),
        None => json!({"type": "text", "text": format!("[@image]({url})")}),
    };

    Some(json!({"sessionUpdate": "user_message_chunk", "content": content}))
}

/// A plan update from `turn/plan/updated`.
pub(super) fn plan(params: &Value) -> Value {
    let entries: Vec<Value> = params["plan"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|step| {
            let status = match step["status"].as_str() {
                Some("inProgress") => "in_progress",
                Some("completed") => "completed",
                _ => "pending",
            };
            json!({"content": step["step"], "priority": "medium", "status": status})
        })
        .collect();
    json!({"sessionUpdate": "plan", "entries": entries})
}

/// A command's tool call as it starts: a read, search or listing by what it
/// does, any other command by its command line.
fn command(item: &Value) -> Value {
    let action = single_action(item);
    let path = action.and_then(|action| action["path"].as_str());
    let mut call = match action.and_then(|action| action["type"].as_str()) {
        Some("read") => json!({
            "kind": "read",
            "title": format!("Read file '{}'", path.unwrap_or_default()),
            "locations": path.map(|path| json!({"path": path})).into_iter().collect::<Vec<_>>(),
        }),
        Some("search") => {
            json!({"kind": "search", "title": search_title(action.and_then(|a| a["query"].as_str()), path)})
        }
        Some("listFiles") => json!({
            "kind": "read",
            "title": path.map_or_else(|| "List files".to_owned(), |path| format!("List files in '{path}'")),
        }),
        _ => {
            let line = action
                .and_then(|action| action["command"].as_str())
                .unwrap_or(text(item, "command"));
            json!({
                "kind": "execute",
                "title": strip_shell(line),
                "rawInput": {"command": line, "cwd": item["cwd"]},
            })
        }
    };

    if let Some(name) = match item["source"].as_str() {
        Some("unifiedExecStartup") => Some("exec_command"),
        Some("unifiedExecInteraction") => Some("write_stdin"),
        _ => None,
    } {
        call["name"] = json!(name);
    }

    call
}

/// A command's end: its output, and the exit code of one that is no plain
/// command line.
fn command_end(item: &Value) -> Value {
    let output = item["aggregatedOutput"]
        .as_str()
        .filter(|output| !output.is_empty());
    let mut end = json!({"content": output.map(text_content).into_iter().collect::<Vec<_>>()});
    let terminal = single_action(item).is_none_or(|action| action["type"] == "unknown");

    if !terminal {
        end["rawOutput"] = json!({"exit_code": item["exitCode"]});
    }

    end
}

/// The command's one action, when it does exactly one thing.
fn single_action(item: &Value) -> Option<&Value> {
    match item["commandActions"].as_array().map(Vec::as_slice) {
        Some([action]) => Some(action),
        _ => None,
    }
}

fn search_title(query: Option<&str>, path: Option<&str>) -> String {
    match (query, path) {
        (Some(query), Some(path)) => format!("Search for '{query}' in {path}"),
        (Some(query), None) => format!("Search for '{query}'"),
        (None, Some(path)) => format!("Search in '{path}'"),
        (None, None) => "Search".to_owned(),
    }
}

/// `line` without the `bash -lc '…'` Codex wraps it in.
pub(super) fn strip_shell(line: &str) -> String {
    static SHELL: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^(?:/(?:[^\s/]+/)*)?(?:bash|zsh|sh)\s+(?:-[lc]+\s+)?")
            .expect("a valid pattern")
    });
    let Some(shell) = SHELL.find(line) else {
        return line.to_owned();
    };
    let rest = &line[shell.end()..];

    if rest.starts_with('-') {
        return line.to_owned();
    }

    static SINGLE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^'([^']*)'$").expect("a valid pattern"));
    static DOUBLE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"^"((?:[^"\\]|\\[\s\S])*)"$"#).expect("a valid pattern"));
    static ESCAPE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"\\([$`"\\\n])"#).expect("a valid pattern"));

    if let Some(inner) = SINGLE.captures(rest) {
        return inner[1].to_owned();
    }

    match DOUBLE.captures(rest) {
        Some(inner) => ESCAPE
            .replace_all(&inner[1], |escape: &regex::Captures| match &escape[1] {
                "\n" => String::new(),
                kept => kept.to_owned(),
            })
            .into_owned(),
        None => rest.to_owned(),
    }
}

fn web_search_title(item: &Value) -> String {
    let action = &item["action"];
    let query = item["query"].as_str().filter(|query| !query.is_empty());

    match action["type"].as_str() {
        Some("openPage") => action["url"]
            .as_str()
            .map_or_else(|| "Open page".to_owned(), |url| format!("Open page: {url}")),
        Some("findInPage") => {
            let pattern = action["pattern"]
                .as_str()
                .map(|p| format!(" for '{p}'"))
                .unwrap_or_default();
            let url = action["url"]
                .as_str()
                .map(|url| format!(" in {url}"))
                .unwrap_or_default();
            format!("Find in page{pattern}{url}")
        }
        _ => {
            let queries: Vec<&str> = action["queries"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .filter(|query| !query.is_empty())
                .collect();
            let joined = queries.join(", ");
            let query = action["query"]
                .as_str()
                .or((!joined.is_empty()).then_some(joined.as_str()))
                .or(query);
            query.map_or_else(
                || "Web search".to_owned(),
                |query| format!("Web search: {query}"),
            )
        }
    }
}

/// The input a subagent call shows, which the node reads its threads from.
fn collab_input(item: &Value) -> Value {
    json!({
        "prompt": item["prompt"],
        "senderThreadId": item["senderThreadId"],
        "receiverThreadIds": item["receiverThreadIds"],
        "agentsStates": item["agentsStates"],
        "model": item["model"],
        "reasoningEffort": item["reasoningEffort"],
        "status": item["status"],
    })
}

/// A file change's diffs: a new or deleted file whole, an edited one hunk
/// by hunk.
fn diffs(change: &Value) -> Vec<Value> {
    let Some(path) = change["path"].as_str() else {
        return Vec::new();
    };
    let diff = change["diff"].as_str().unwrap_or_default();

    match change["kind"]["type"].as_str() {
        Some("add") => {
            vec![json!({"type": "diff", "path": path, "oldText": null, "newText": diff})]
        }
        Some("delete") => {
            vec![json!({"type": "diff", "path": path, "oldText": diff, "newText": ""})]
        }
        _ => {
            let path = change["kind"]["move_path"].as_str().unwrap_or(path);
            hunks(diff)
                .into_iter()
                .map(|(old, new)| json!({"type": "diff", "path": path, "oldText": old, "newText": new}))
                .collect()
        }
    }
}

/// The text before and after each hunk of the unified diff `diff`.
fn hunks(diff: &str) -> Vec<(String, String)> {
    // `diffy` reads a patch with its file headers; Codex sends the hunks.
    let patch = match diff.starts_with("--- ") {
        true => diff.to_owned(),
        false => format!("--- a\n+++ b\n{diff}"),
    };
    let Ok(patch) = diffy::Patch::from_str(&patch) else {
        log::warn!("a file change's diff does not read");
        return Vec::new();
    };

    patch
        .hunks()
        .iter()
        .map(|hunk| {
            let (mut old, mut new) = (String::new(), String::new());
            for line in hunk.lines() {
                match line {
                    diffy::Line::Context(text) => {
                        old.push_str(text);
                        new.push_str(text);
                    }
                    diffy::Line::Delete(text) => old.push_str(text),
                    diffy::Line::Insert(text) => new.push_str(text),
                }
            }
            (old, new)
        })
        .collect()
}

/// The text of an MCP tool's result.
fn mcp_text(result: &Value) -> Option<String> {
    let texts: Vec<&str> = result["content"]
        .as_array()?
        .iter()
        .filter_map(|content| content["text"].as_str())
        .collect();
    (!texts.is_empty()).then(|| texts.join("\n"))
}

fn text_update(kind: &str, text: &str) -> Option<Value> {
    (!text.trim().is_empty()).then(|| chunk(kind, text))
}

fn text_content(text: &str) -> Value {
    json!({"type": "content", "content": {"type": "text", "text": text}})
}

fn status(status: &Value) -> &'static str {
    match status.as_str() {
        Some("completed") => "completed",
        Some("failed" | "declined" | "interrupted") => "failed",
        _ => "in_progress",
    }
}

fn text<'a>(item: &'a Value, key: &str) -> &'a str {
    item[key].as_str().unwrap_or_default()
}
