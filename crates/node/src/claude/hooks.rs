//! Claude Code hooks that tell the node when a session ends a turn or waits
//! for the user. `beep-node hooks install` adds them to Claude Code's
//! settings; each one runs `beep-node hook`, which hands the event to the
//! running node over a local socket (`socket`) and always exits 0,
//! so a node that is not running never gets in Claude Code's way.

use super::socket;
use super::watch::MAX_MESSAGE;
use crate::agents::cut;
use anyhow::{Context, Result, bail};
use beep_proto::node::agent::{Activity, ActivityKind};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::{
    env, fs,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{io::AsyncWriteExt, time::timeout};

/// The events the hooks are installed for.
const EVENTS: &[&str] = &["Stop", "Notification", "SessionStart", "SessionEnd"];
/// The socket in the node's folder.
const SOCKET: &str = "hooks.sock";
/// How long Claude Code lets a hook run, in seconds.
const HOOK_TIMEOUT_SECS: u64 = 5;
/// How long the hook tries to reach the node.
const SEND_TIMEOUT: Duration = Duration::from_secs(2);
/// The most hook input read.
const MAX_INPUT: u64 = 1024 * 1024;
/// What marks a hook as this node's.
const MARK: &str = "beep-node";

pub fn socket_path(dir: &Path) -> PathBuf {
    dir.join(SOCKET)
}

/// Claude Code's user settings file.
pub fn user_settings() -> Result<PathBuf> {
    let root = match env::var_os("CLAUDE_CONFIG_DIR") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => dirs::home_dir()
            .context("this machine has no home folder")?
            .join(".claude"),
    };
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

/// Adds a hook running `command` for each event that has none; returns the
/// events it added to.
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
    for event in EVENTS {
        let groups = hooks
            .entry(*event)
            .or_insert_with(|| Value::Array(Vec::new()));
        let Some(groups) = groups.as_array_mut() else {
            bail!("`hooks.{event}` in the settings is not a list");
        };
        let present = groups
            .iter()
            .filter_map(|group| group["hooks"].as_array())
            .flatten()
            .any(is_ours);
        if !present {
            groups.push(group(command));
            added.push(*event);
        }
    }
    Ok(added)
}

/// One matcher group holding this node's hook.
fn group(command: &str) -> Value {
    json!({
        "hooks": [{
            "type": "command",
            "command": command,
            "timeout": HOOK_TIMEOUT_SECS,
            "async": true
        }]
    })
}

/// Removes this node's hooks, then groups and events left empty; returns
/// how many hooks it removed.
fn remove(settings: &mut Value) -> usize {
    let Some(hooks) = settings.get_mut("hooks").and_then(Value::as_object_mut) else {
        return 0;
    };
    let mut removed = 0;
    for event in EVENTS {
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
        .copied()
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
        "Added to {}, under \"hooks\", for {}:\n{}\n",
        settings_path.display(),
        added.join(", "),
        serde_json::to_string_pretty(&group(command))?
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum HookEvent {
    Stop,
    Notification,
    SessionStart,
    SessionEnd,
    #[serde(other)]
    Other,
}

/// Notifications that mean the session waits for the user.
const WAITING: &[&str] = &[
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
        Some(Self {
            event,
            session_id,
            message: message.map(|text| cut(text.trim(), MAX_MESSAGE)),
            notification_type: input["notification_type"].as_str().map(str::to_owned),
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
            HookEvent::Other => return None,
        };
        Some(Activity {
            session: self.session_id.clone(),
            kind,
            message: self.message.clone().unwrap_or_default(),
            from_hook: true,
        })
    }
}

/// `beep-node hook`: reads the hook input on stdin and hands it to the
/// node. Never fails: Claude Code would show the user an error.
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
    if let Err(e) = send(&socket_path(dir), &message).await {
        log::debug!("the node did not take the hook: {e:#}");
    }
}

pub(super) async fn send(socket: &Path, message: &HookMessage) -> Result<()> {
    let mut line = serde_json::to_string(message)?;
    line.push('\n');
    timeout(SEND_TIMEOUT, async {
        let mut stream = socket::connect(socket).await?;
        stream.write_all(line.as_bytes()).await?;
        stream.shutdown().await
    })
    .await
    .context("the node did not answer")??;
    Ok(())
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
        assert_eq!(add(&mut settings, COMMAND).unwrap(), EVENTS);
        assert!(add(&mut settings, COMMAND).unwrap().is_empty());
        assert_eq!(settings["hooks"]["Stop"].as_array().unwrap().len(), 2);
        assert_eq!(settings["model"], "opus");
        assert_eq!(installed(&settings), EVENTS);
        assert_eq!(remove(&mut settings), 4);
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

    #[test]
    fn hook_input_reads_as_activity() {
        let stop = json!({
            "session_id": "0f3c2a9e-1111-4222-8333-944455556666",
            "transcript_path": "/srv/home/.claude/projects/-srv-project/0f3c2a9e-1111-4222-8333-944455556666.jsonl",
            "cwd": "/srv/project",
            "hook_event_name": "Stop",
            "stop_hook_active": false,
            "last_assistant_message": "Done: the tests pass."
        });
        let activity = HookMessage::from_input(&stop).unwrap().activity().unwrap();
        assert_eq!(activity.kind, ActivityKind::TurnDone);
        assert_eq!(activity.message, "Done: the tests pass.");
        let asking = json!({
            "session_id": "s",
            "hook_event_name": "Notification",
            "notification_type": "permission_prompt",
            "message": "Claude needs your permission to use Bash"
        });
        let activity = HookMessage::from_input(&asking)
            .unwrap()
            .activity()
            .unwrap();
        assert_eq!(activity.kind, ActivityKind::NeedsInput);
        let signed_in = json!({
            "session_id": "s",
            "hook_event_name": "Notification",
            "notification_type": "auth_success"
        });
        assert!(
            HookMessage::from_input(&signed_in)
                .unwrap()
                .activity()
                .is_none()
        );
        let tool = json!({ "session_id": "s", "hook_event_name": "PreToolUse" });
        assert!(HookMessage::from_input(&tool).unwrap().activity().is_none());
    }
}
