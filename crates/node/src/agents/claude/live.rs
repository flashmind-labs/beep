//! What each external Claude Code session does now, as its hooks report
//! it, and what Beep queued for it (`Live`). The next hook to fire delivers a
//! queued message.

use super::hooks::{HookEvent, HookMessage, WAITING};
use beep_proto::node::agent::{ExternalSession, LiveState, LiveStatus, LiveTool, PlanEntry, Steer};
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Mutex, time::Instant};

/// The most sessions tracked; the one heard from least recently goes.
const MAX_TRACKED: usize = 64;
/// The most written files kept per session.
const MAX_FILES: usize = 50;
/// What a stopped tool call tells Claude.
const STOPPED: &str = "The user asked you to stop.";

/// Each tracked session's state and queued message. A queued stop denies
/// every tool call until the turn ends. A session idle at its prompt fires
/// no hook, so nothing can be queued for it.
#[derive(Default)]
pub(crate) struct Live(Mutex<HashMap<String, Tracked>>);

struct Tracked {
    state: LiveState,
    waiting_for: String,
    tool: Option<Running>,
    error: Option<String>,
    todos: Vec<PlanEntry>,
    files: Vec<String>,
    last_message: String,
    queued: Option<Steer>,
    heard: Instant,
}

struct Running {
    name: String,
    detail: String,
    since: Instant,
}

impl Tracked {
    fn new() -> Self {
        Self {
            state: LiveState::Unknown,
            waiting_for: String::new(),
            tool: None,
            error: None,
            todos: Vec::new(),
            files: Vec::new(),
            last_message: String::new(),
            queued: None,
            heard: Instant::now(),
        }
    }
}

/// The hook output that blocks with `reason`: for `PostToolUse` Claude reads
/// it after the tool call, for `Stop` it keeps working.
fn block(reason: &str) -> Value {
    json!({ "decision": "block", "reason": reason })
}

fn from_user(text: &str) -> String {
    format!("The user sent this through Beep: {text}")
}

fn deny(reason: &str) -> Value {
    json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": reason
        }
    })
}

impl Live {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Tracked>> {
        // The map stays whole even if a holder panicked.
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Takes in one hook event; returns what the hook prints for Claude
    /// Code, if anything.
    pub(super) fn hear(&self, message: &HookMessage) -> Option<Value> {
        let mut sessions = self.lock();
        if !sessions.contains_key(&message.session_id)
            && sessions.len() >= MAX_TRACKED
            && let Some(oldest) = sessions
                .iter()
                .min_by_key(|(_, tracked)| tracked.heard)
                .map(|(id, _)| id.clone())
        {
            sessions.remove(&oldest);
        }
        let tracked = sessions
            .entry(message.session_id.clone())
            .or_insert_with(Tracked::new);
        tracked.heard = Instant::now();
        match message.event {
            HookEvent::SessionStart => {
                tracked.state = LiveState::Idle;
                tracked.tool = None;
                None
            }
            HookEvent::UserPromptSubmit => {
                tracked.state = LiveState::Working;
                tracked.error = None;
                None
            }
            HookEvent::PreToolUse => {
                tracked.state = LiveState::Working;
                tracked.tool = Some(Running {
                    name: message.tool.clone().unwrap_or_default(),
                    detail: message.detail.clone().unwrap_or_default(),
                    since: Instant::now(),
                });
                if let Some(todos) = &message.todos {
                    tracked.todos = todos.clone();
                }
                matches!(tracked.queued, Some(Steer::Stop {})).then(|| deny(STOPPED))
            }
            HookEvent::PostToolUse | HookEvent::PostToolUseFailure => {
                tracked.state = LiveState::Working;
                tracked.tool = None;
                if let Some(file) = &message.file
                    && !tracked.files.contains(file)
                    && tracked.files.len() < MAX_FILES
                {
                    tracked.files.push(file.clone());
                }
                if message.error.is_some() {
                    tracked.error = message.error.clone();
                }
                match tracked.queued.take()? {
                    Steer::Message { text } => Some(block(&from_user(&text))),
                    // A stop holds until the turn ends.
                    stop => {
                        tracked.queued = Some(stop);
                        Some(block(STOPPED))
                    }
                }
            }
            HookEvent::Stop => {
                tracked.tool = None;
                if let Some(Steer::Message { text }) = tracked.queued.take() {
                    tracked.state = LiveState::Working;
                    return Some(block(&from_user(&text)));
                }
                tracked.state = LiveState::Idle;
                if let Some(text) = &message.message {
                    tracked.last_message = text.clone();
                }
                None
            }
            HookEvent::Notification => {
                match message.notification_type.as_deref() {
                    Some("idle_prompt") => tracked.state = LiveState::Idle,
                    Some(kind) if WAITING.contains(&kind) => {
                        tracked.state = LiveState::Asking;
                        tracked.waiting_for = message.message.clone().unwrap_or_default();
                    }
                    _ => {}
                }
                None
            }
            HookEvent::SessionEnd => {
                tracked.state = LiveState::Ended;
                tracked.tool = None;
                tracked.queued = None;
                None
            }
            HookEvent::Other => None,
        }
    }

    /// Whether a hook spoke for `session` since the node started.
    pub(super) fn knows(&self, session: &str) -> bool {
        self.lock().contains_key(session)
    }

    /// `found` as its hooks report it.
    pub(super) fn status(&self, mut found: ExternalSession) -> LiveStatus {
        let sessions = self.lock();
        let Some(tracked) = sessions.get(&found.id) else {
            return LiveStatus {
                session: found,
                ..LiveStatus::default()
            };
        };
        if !tracked.last_message.is_empty() {
            found.last_message = tracked.last_message.clone();
        }
        LiveStatus {
            session: found,
            state: tracked.state,
            waiting_for: if tracked.state == LiveState::Asking {
                tracked.waiting_for.clone()
            } else {
                String::new()
            },
            tool: tracked.tool.as_ref().map(|running| LiveTool {
                name: running.name.clone(),
                detail: running.detail.clone(),
                secs: running.since.elapsed().as_secs(),
            }),
            error: tracked.error.clone(),
            todos: tracked.todos.clone(),
            files: tracked.files.clone(),
            queued: tracked.queued.clone(),
        }
    }

    /// Queues `steer` for `session`'s next hook; the error says why it
    /// cannot be.
    pub(super) fn steer(&self, session: &str, steer: Steer) -> Result<(), String> {
        let mut sessions = self.lock();
        let Some(tracked) = sessions.get_mut(session) else {
            return Err(format!(
                "no hook reported on session {session} since the node started, so nothing \
                 can reach it. It needs `beep-node hooks install` on this machine and a \
                 restart of the session, or it has not done anything since."
            ));
        };
        match tracked.state {
            LiveState::Working | LiveState::Asking => {}
            LiveState::Idle => {
                return Err(format!(
                    "session {session} is idle at its prompt, so no hook is left to deliver \
                     anything. Only the user can prompt it there, or exit it so Beep can take \
                     it over."
                ));
            }
            _ => return Err(format!("session {session} has ended")),
        }
        tracked.queued = match (tracked.queued.take(), steer) {
            (Some(Steer::Message { text: before }), Steer::Message { text }) => {
                Some(Steer::Message {
                    text: format!("{before}\n\n{text}"),
                })
            }
            (_, steer) => Some(steer),
        };
        Ok(())
    }
}
