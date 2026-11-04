//! Claude Code hooks that tell the node what a session does: each tool call
//! as it starts and ends, a turn's end, a wait for the user.
//! `beep-node hooks install` adds them to Claude Code's settings; each one
//! runs `beep-node hook`, which hands the event to the running node over a
//! local socket (`socket`) and prints what the node answers, such as a
//! message Beep queued for the session (see `super::live`). It always exits
//! 0 within a second, so a node that is not running never gets in Claude
//! Code's way.

use super::watch::MAX_MESSAGE;
use crate::agents::cut;
use crate::local_socket as socket;
use anyhow::{Context, Result, bail};
use beep_proto::node::agent::{Activity, ActivityKind, PlanEntry, PlanStatus};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    time::timeout,
};

/// The events the hooks are installed for, and whether Claude Code waits
/// for the hook: only those that can deliver what Beep queued.
const EVENTS: &[(&str, bool)] = &[
    ("Stop", true),
    ("PreToolUse", true),
    ("PostToolUse", true),
    ("PostToolUseFailure", true),
    ("UserPromptSubmit", false),
    ("Notification", false),
    ("SessionStart", false),
    ("SessionEnd", false),
];
/// The socket in the node's folder.
const SOCKET: &str = "hooks.sock";
/// How long Claude Code lets a hook it waits for run, in seconds.
const WAITED_TIMEOUT_SECS: u64 = 2;
/// How long Claude Code lets a background hook run, in seconds.
const HOOK_TIMEOUT_SECS: u64 = 5;
/// How long the hook tries to reach the node and hear back.
const SEND_TIMEOUT: Duration = Duration::from_secs(1);
/// The most hook input read.
const MAX_INPUT: u64 = 1024 * 1024;
/// The most of the node's answer read.
const MAX_ANSWER: u64 = 64 * 1024;
/// The longest tool detail and error kept.
const MAX_DETAIL: usize = 300;
/// What marks a hook as this node's.
const MARK: &str = "beep-node";

pub fn socket_path(dir: &Path) -> PathBuf {
    dir.join(SOCKET)
}

/// Claude Code's user settings file.
pub fn user_settings() -> Result<PathBuf> {
    let root = super::config_dir().context("this machine has no home folder")?;
    Ok(root.join("settings.json"))
}

/// The command a hook runs: this program, with the node's folder.
pub fn hook_command(exe: &Path, dir: &Path) -> String {
    format!(
        "{} --dir {} hook",
        quote(&exe.to_string_lossy()),
        quote(&dir.to_string_lossy())
    )
}

/// `text` quoted for a POSIX shell.
fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

fn is_ours(handler: &Value) -> bool {
    handler["command"]
        .as_str()
        .is_some_and(|command| command.contains(MARK) && command.ends_with(" hook"))
}

/// Adds a hook running `command` for each event that has none, and brings
/// this node's older ones up to date; returns the events it changed.
fn add(settings: &mut Value, command: &str) -> Result<Vec<&'static str>> {
    if settings.is_null() {
        *settings = Value::Object(Map::new());
    }
    let Some(root) = settings.as_object_mut() else {
        bail!("the settings file is not a JSON object");
    };
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()));
    let Some(hooks) = hooks.as_object_mut() else {
        bail!("`hooks` in the settings is not an object");
    };
    let mut added = Vec::new();
    for &(event, waited) in EVENTS {
        let wanted = handler(command, waited);
        let groups = hooks
            .entry(event)
            .or_insert_with(|| Value::Array(Vec::new()));
        let Some(groups) = groups.as_array_mut() else {
            bail!("`hooks.{event}` in the settings is not a list");
        };
        let mut present = false;
        let mut changed = false;
        let ours = groups
            .iter_mut()
            .filter_map(|group| group.get_mut("hooks").and_then(Value::as_array_mut))
            .flatten()
            .filter(|handler| is_ours(handler));
        for handler in ours {
            present = true;
            if *handler != wanted {
                *handler = wanted.clone();
                changed = true;
            }
        }
        if !present {
            groups.push(json!({ "hooks": [wanted] }));
            changed = true;
        }
        if changed {
            added.push(event);
        }
    }
    Ok(added)
}

/// This node's hook for an event Claude Code waits for, or not.
fn handler(command: &str, waited: bool) -> Value {
    if waited {
        json!({ "type": "command", "command": command, "timeout": WAITED_TIMEOUT_SECS })
    } else {
        json!({
            "type": "command",
            "command": command,
            "timeout": HOOK_TIMEOUT_SECS,
            "async": true
        })
    }
}

/// Removes this node's hooks, then groups and events left empty; returns
/// how many hooks it removed.
fn remove(settings: &mut Value) -> usize {
    let Some(hooks) = settings.get_mut("hooks").and_then(Value::as_object_mut) else {
        return 0;
    };
    let mut removed = 0;
    for (event, _) in EVENTS {
        let Some(groups) = hooks.get_mut(*event).and_then(Value::as_array_mut) else {
            continue;
        };
        for group in groups.iter_mut() {
            if let Some(handlers) = group.get_mut("hooks").and_then(Value::as_array_mut) {
                let before = handlers.len();
                handlers.retain(|handler| !is_ours(handler));
                removed += before - handlers.len();
            }
        }
        groups.retain(|group| group["hooks"].as_array().is_none_or(|h| !h.is_empty()));
        if groups.is_empty() {
            hooks.remove(*event);
        }
    }
    if hooks.is_empty()
        && let Some(root) = settings.as_object_mut()
    {
        root.remove("hooks");
    }
    removed
}

/// The events this node has a hook for.
fn installed(settings: &Value) -> Vec<&'static str> {
    EVENTS
        .iter()
        .map(|&(event, _)| event)
        .filter(|event| {
            settings["hooks"][event]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|group| group["hooks"].as_array())
                .flatten()
                .any(is_ours)
        })
        .collect()
}

fn read_settings(path: &Path) -> Result<Value> {
    match fs::read_to_string(path) {
        Ok(text) if text.trim().is_empty() => Ok(Value::Null),
        Ok(text) => serde_json::from_str(&text)
            .with_context(|| format!("{} is not valid JSON; fix it first", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Value::Null),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

/// Saves `settings` to `path` through a temporary file, after copying the
/// old file beside it. Returns the copy's path.
fn write_settings(path: &Path, settings: &Value) -> Result<Option<PathBuf>> {
    let backup = if path.exists() {
        let backup = path.with_extension("json.beep-backup");
        fs::copy(path, &backup).with_context(|| format!("backing up {}", path.display()))?;
        Some(backup)
    } else {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        None
    };
    let temp = path.with_extension("json.beep-tmp");
    let mut text = serde_json::to_string_pretty(settings)?;
    text.push('\n');
    fs::write(&temp, text).with_context(|| format!("writing {}", temp.display()))?;
    fs::rename(&temp, path).with_context(|| format!("replacing {}", path.display()))?;
    Ok(backup)
}

/// `beep-node hooks install`: adds the hooks to `settings` and says what
/// changed.
pub fn install(settings_path: &Path, command: &str) -> Result<String> {
    let mut settings = read_settings(settings_path)?;
    let added = add(&mut settings, command)?;
    if added.is_empty() {
        return Ok(format!(
            "The hooks are already in {}; nothing changed.",
            settings_path.display()
        ));
    }
    let backup = write_settings(settings_path, &settings)?;
    let mut out = format!(
        "Added to {}, under \"hooks\", for {}: {command}\nClaude Code waits for it, at most \
         {WAITED_TIMEOUT_SECS}s, on tool calls and turn ends, so Beep can steer the session.\n",
        settings_path.display(),
        added.join(", "),
    );
    if let Some(backup) = backup {
        out.push_str(&format!(
            "The file was rewritten with its keys sorted; the old one is at {}.\n",
            backup.display()
        ));
    }
    out.push_str(
        "Claude Code reads hooks when a session starts, so sessions open now pick them up \
         once restarted. Remove them with `beep-node hooks uninstall`.",
    );
    Ok(out)
}

/// `beep-node hooks uninstall`.
pub fn uninstall(settings_path: &Path) -> Result<String> {
    let mut settings = read_settings(settings_path)?;
    let removed = remove(&mut settings);
    if removed == 0 {
        return Ok(format!(
            "No beep-node hooks in {}; nothing changed.",
            settings_path.display()
        ));
    }
    let backup = write_settings(settings_path, &settings)?;
    let mut out = format!(
        "Removed {removed} beep-node hooks from {}.",
        settings_path.display()
    );
    if let Some(backup) = backup {
        out.push_str(&format!(" The old file is at {}.", backup.display()));
    }
    Ok(out)
}

/// `beep-node hooks status`.
pub fn status(settings_path: &Path, dir: &Path) -> Result<String> {
    let settings = read_settings(settings_path)?;
    let events = installed(&settings);
    let hooks = if events.is_empty() {
        format!(
            "No beep-node hooks in {}. Without them the node only sees turns end, by \
             watching transcripts; `beep-node hooks install` adds them.",
            settings_path.display()
        )
    } else {
        format!(
            "beep-node hooks in {} for {}.",
            settings_path.display(),
            events.join(", ")
        )
    };
    let socket = socket_path(dir);
    let listening = if socket::is_listening(&socket) {
        "The node is running and listens for them."
    } else {
        "The node is not running here (`beep-node run`), so hook events are dropped."
    };
    Ok(format!("{hooks}\n{listening}"))
}

/// What a hook hands the node: one JSON line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct HookMessage {
    pub(super) event: HookEvent,
    pub(super) session_id: String,
    #[serde(default)]
    pub(super) message: Option<String>,
    #[serde(default)]
    pub(super) notification_type: Option<String>,
    /// The tool a tool event is about.
    #[serde(default)]
    pub(super) tool: Option<String>,
    /// What the tool works on: a command, a file, a pattern.
    #[serde(default)]
    pub(super) detail: Option<String>,
    /// The file a tool wrote.
    #[serde(default)]
    pub(super) file: Option<String>,
    /// The todo list a `TodoWrite` call sets.
    #[serde(default)]
    pub(super) todos: Option<Vec<PlanEntry>>,
    /// Why a tool call failed.
    #[serde(default)]
    pub(super) error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum HookEvent {
    Stop,
    Notification,
    SessionStart,
    SessionEnd,
    PreToolUse,
    PostToolUse,
    PostToolUseFailure,
    UserPromptSubmit,
    #[serde(other)]
    Other,
}

/// Tools whose `file_path` (or `notebook_path`) is a file they write.
const WRITERS: &[&str] = &["Write", "Edit", "MultiEdit", "NotebookEdit"];

/// Notifications that mean the session waits for the user.
pub(super) const WAITING: &[&str] = &[
    "permission_prompt",
    "idle_prompt",
    "elicitation_dialog",
    "elicitation_url_dialog",
    "agent_needs_input",
];

impl HookMessage {
    /// Reads Claude Code's hook input.
    pub(super) fn from_input(input: &Value) -> Option<Self> {
        let event = serde_json::from_value(input["hook_event_name"].clone()).ok()?;
        let session_id = input["session_id"].as_str()?.to_owned();
        let message = match event {
            HookEvent::Stop => input["last_assistant_message"].as_str(),
            _ => input["message"].as_str(),
        };
        let tool = input["tool_name"].as_str();
        let args = &input["tool_input"];
        let file = tool
            .filter(|tool| event == HookEvent::PostToolUse && WRITERS.contains(tool))
            .and_then(|_| {
                args["file_path"]
                    .as_str()
                    .or(args["notebook_path"].as_str())
            })
            .map(str::to_owned);
        let todos = (event == HookEvent::PreToolUse && tool == Some("TodoWrite"))
            .then(|| args["todos"].as_array())
            .flatten()
            .map(|todos| todos.iter().filter_map(todo).collect());
        let error = (event == HookEvent::PostToolUseFailure
            && input["is_interrupt"].as_bool() != Some(true))
        .then(|| input["error"].as_str())
        .flatten()
        .map(|text| cut(text.trim(), MAX_DETAIL));
        Some(Self {
            event,
            session_id,
            message: message.map(|text| cut(text.trim(), MAX_MESSAGE)),
            notification_type: input["notification_type"].as_str().map(str::to_owned),
            tool: tool.map(str::to_owned),
            detail: tool_detail(args).map(|text| cut(text.trim(), MAX_DETAIL)),
            file,
            todos,
            error,
        })
    }

    /// What it means for a follower, `None` for events nobody follows.
    pub(super) fn activity(&self) -> Option<Activity> {
        let kind = match self.event {
            HookEvent::Stop => ActivityKind::TurnDone,
            HookEvent::Notification => {
                let kind = self.notification_type.as_deref()?;
                if !WAITING.contains(&kind) {
                    return None;
                }
                ActivityKind::NeedsInput
            }
            HookEvent::SessionStart => ActivityKind::Started,
            HookEvent::SessionEnd => ActivityKind::Ended,
            HookEvent::PreToolUse
            | HookEvent::PostToolUse
            | HookEvent::PostToolUseFailure
            | HookEvent::UserPromptSubmit
            | HookEvent::Other => return None,
        };
        Some(Activity {
            session: self.session_id.clone(),
            kind,
            message: self.message.clone().unwrap_or_default(),
            from_hook: true,
        })
    }
}

/// What a tool call works on, from its input.
fn tool_detail(args: &Value) -> Option<&str> {
    [
        "description",
        "command",
        "file_path",
        "notebook_path",
        "pattern",
        "url",
        "query",
    ]
    .iter()
    .find_map(|key| args[key].as_str())
    .filter(|text| !text.trim().is_empty())
}

/// One entry of a `TodoWrite` list.
fn todo(item: &Value) -> Option<PlanEntry> {
    let status = match item["status"].as_str() {
        Some("completed") => PlanStatus::Completed,
        Some("in_progress") => PlanStatus::InProgress,
        _ => PlanStatus::Pending,
    };
    Some(PlanEntry {
        content: cut(item["content"].as_str()?.trim(), MAX_DETAIL),
        status,
    })
}

/// `beep-node hook`: reads the hook input on stdin, hands it to the node
/// and prints the node's answer for Claude Code. Never fails: Claude Code
/// would show the user an error.
pub async fn forward(dir: &Path) {
    let mut input = String::new();
    if std::io::stdin()
        .take(MAX_INPUT)
        .read_to_string(&mut input)
        .is_err()
    {
        return;
    }
    let Some(message) = serde_json::from_str(&input)
        .ok()
        .and_then(|input: Value| HookMessage::from_input(&input))
    else {
        return;
    };
    match send(&socket_path(dir), &message).await {
        Ok(answer) if !answer.is_empty() => {
            let mut out = std::io::stdout().lock();
            if let Err(e) = out.write_all(answer.as_bytes()).and_then(|()| out.flush()) {
                log::debug!("could not hand Claude Code the node's answer: {e}");
            }
        }
        Ok(_) => {}
        Err(e) => log::debug!("the node did not take the hook: {e:#}"),
    }
}

/// Hands `message` to the node and returns its answer: the JSON the hook
/// prints, empty when it has none. A node from before answers nothing.
pub(super) async fn send(socket: &Path, message: &HookMessage) -> Result<String> {
    let mut line = serde_json::to_string(message)?;
    line.push('\n');
    let answer = timeout(SEND_TIMEOUT, async {
        let mut stream = socket::connect(socket).await?;
        stream.write_all(line.as_bytes()).await?;
        stream.shutdown().await?;
        let mut answer = String::new();
        (&mut stream)
            .take(MAX_ANSWER)
            .read_to_string(&mut answer)
            .await?;
        Ok::<_, std::io::Error>(answer)
    })
    .await
    .context("the node did not answer")??;
    Ok(answer.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMMAND: &str = "'/opt/beep/beep-node' --dir '/srv/node' hook";

    #[test]
    fn install_adds_each_event_once_and_keeps_other_hooks() {
        let mut settings = json!({
            "model": "opus",
            "hooks": {
                "Stop": [{ "hooks": [{ "type": "command", "command": "say done" }] }]
            }
        });
        let names: Vec<&str> = EVENTS.iter().map(|&(event, _)| event).collect();
        assert_eq!(add(&mut settings, COMMAND).unwrap(), names);
        assert!(add(&mut settings, COMMAND).unwrap().is_empty());
        assert_eq!(settings["hooks"]["Stop"].as_array().unwrap().len(), 2);
        assert_eq!(settings["model"], "opus");
        assert_eq!(installed(&settings), names);
        // An older install's background Stop hook becomes one Claude Code
        // waits for.
        settings["hooks"]["Stop"][1]["hooks"][0] = handler(COMMAND, false);
        assert_eq!(add(&mut settings, COMMAND).unwrap(), ["Stop"]);
        assert_eq!(
            settings["hooks"]["Stop"][1]["hooks"][0],
            handler(COMMAND, true)
        );
        assert_eq!(remove(&mut settings), EVENTS.len());
        assert_eq!(
            settings,
            json!({
                "model": "opus",
                "hooks": {
                    "Stop": [{ "hooks": [{ "type": "command", "command": "say done" }] }]
                }
            })
        );
        let mut empty = Value::Null;
        add(&mut empty, COMMAND).unwrap();
        remove(&mut empty);
        assert_eq!(empty, json!({}));
    }

    #[test]
    fn settings_of_the_wrong_shape_are_refused() {
        assert!(add(&mut json!([1]), COMMAND).is_err());
        assert!(add(&mut json!({ "hooks": { "Stop": {} } }), COMMAND).is_err());
    }

    #[test]
    fn the_command_quotes_its_paths() {
        assert_eq!(
            hook_command(Path::new("/opt/it's/beep-node"), Path::new("/srv/node")),
            r"'/opt/it'\''s/beep-node' --dir '/srv/node' hook"
        );
        assert!(is_ours(
            &json!({ "command": hook_command(Path::new("/opt/beep-node"), Path::new("/x")) })
        ));
    }
}
