//! Handing the main window a view to show: what the bar, the tray, a
//! notice or another window asks it to open. Entry point: `open_on`.

use super::window::{LABEL, open};
use crate::error::Result;
use beep_proto::AgentNoticeKey;
use serde::{Deserialize, Serialize};
use std::sync::{Mutex, PoisonError};
use tauri::{AppHandle, Emitter, Runtime, State};
use uuid::Uuid;

/// A view of the window that the bar, the tray or another window can hand
/// over to. `Chat { session: None }` is a new chat.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CenterTarget {
    Home,
    Chat {
        session: Option<Uuid>,
    },
    Chats,
    Memories,
    Routines,
    Skills,
    Apps,
    Settings,
    Instructions,
    Group {
        id: String,
    },
    /// A coding agent's session, as a Running now row names it.
    Agent(Box<AgentTarget>),
    Providers,
    /// The Projects page on `project`, or every project with `None`, as a
    /// project window hands over another project.
    Projects {
        project: Option<String>,
    },
    Terminals,
    Research,
    /// One research, opened as a page of the window.
    OpenResearch {
        session: Uuid,
    },
    /// The notifications pane, with alert `session` opened when there is one.
    Notice {
        session: Option<Uuid>,
    },
    /// A new chat that starts with `prompt`, as a project window's ⌘K asks.
    Ask {
        prompt: String,
    },
    /// The Report a problem sheet, over the page on screen.
    Report,
}

/// A coding agent's session to open, shaped as the window's Running now
/// row (`RunningAgent`) so it opens like one.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub(crate) struct AgentTarget {
    kind: String,
    machine_id: String,
    machine_name: String,
    id: String,
    agent: String,
    title: Option<String>,
    cwd: String,
    status: String,
    doing: Option<String>,
    started_at: u64,
    last_activity: u64,
    external: bool,
}

impl AgentTarget {
    /// Agent `agent`'s session `session` on machine `machine_id`, which
    /// waits on a question, boxed as `CenterTarget::Agent` holds it.
    pub(crate) fn asking(
        machine_id: &str,
        machine_name: &str,
        agent: &str,
        session: &str,
        title: Option<&str>,
    ) -> Box<Self> {
        Box::new(Self {
            kind: "agent".into(),
            machine_id: machine_id.to_owned(),
            machine_name: machine_name.to_owned(),
            id: session.to_owned(),
            agent: agent.to_owned(),
            title: title.map(str::to_owned),
            cwd: String::new(),
            status: "asking".into(),
            doing: None,
            started_at: 0,
            last_activity: 0,
            external: false,
        })
    }

    /// The session a coding-agent notice is about: asking while its
    /// question waits, idle once a turn ended.
    pub(crate) fn notified(key: &AgentNoticeKey) -> Box<Self> {
        let mut target = Self::asking(
            &key.machine_id,
            &key.machine_name,
            &key.agent,
            &key.session,
            None,
        );
        if key.question.is_none() {
            target.status = "idle".into();
        }
        target
    }
}

/// The view the window goes to next. It is read once, so a reload does not
/// go back to it.
#[derive(Debug, Default)]
pub(crate) struct Pending(Mutex<Option<CenterTarget>>);

impl Pending {
    fn put(&self, target: CenterTarget) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(target);
    }

    fn take(&self) -> Option<CenterTarget> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).take()
    }
}

/// Opens the window on `target`. A window that is still loading reads it
/// with `center_pending`; one already open hears `center-open`.
pub(crate) fn open_on<R: Runtime>(
    app: &AppHandle<R>,
    pending: &Pending,
    target: CenterTarget,
) -> Result<()> {
    pending.put(target.clone());
    open(app)?;
    // A window still loading reads `Pending` instead, so a lost event only
    // goes to the log.
    if let Err(e) = app.emit_to(LABEL, "center-open", &target) {
        log::warn!("telling the command center what to open: {e}");
    }
    Ok(())
}

#[tauri::command]
pub fn center_open(
    app: AppHandle,
    pending: State<'_, Pending>,
    target: CenterTarget,
) -> Result<()> {
    open_on(&app, &pending, target)
}

/// The view the window was asked to show, once.
#[tauri::command]
pub fn center_pending(pending: State<'_, Pending>) -> Option<CenterTarget> {
    pending.take()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(question: Option<u32>) -> AgentNoticeKey {
        AgentNoticeKey {
            machine_id: "m1".into(),
            agent: "claude".into(),
            session: "s1".into(),
            question,
            machine_name: "build-box".into(),
        }
    }

    #[test]
    fn a_notice_opens_its_agent_session() {
        let asking = AgentTarget::notified(&key(Some(2)));
        assert_eq!(
            asking,
            AgentTarget::asking("m1", "build-box", "claude", "s1", None)
        );
        let done = AgentTarget::notified(&key(None));
        assert_eq!(done.status, "idle");
        assert_eq!(done.id, "s1");
    }

    #[test]
    fn a_project_window_hands_its_targets_to_the_main_window() {
        let read = |json: &str| serde_json::from_str::<CenterTarget>(json).ok();
        assert_eq!(
            read(r#"{"projects":{"project":"org/site"}}"#),
            Some(CenterTarget::Projects {
                project: Some("org/site".into())
            })
        );
        assert_eq!(
            read(r#"{"projects":{"project":null}}"#),
            Some(CenterTarget::Projects { project: None })
        );
        assert_eq!(read(r#""terminals""#), Some(CenterTarget::Terminals));
        assert_eq!(read(r#""research""#), Some(CenterTarget::Research));
        assert_eq!(read(r#""report""#), Some(CenterTarget::Report));
        let session = Uuid::new_v4();
        assert_eq!(
            read(&format!(r#"{{"open_research":{{"session":"{session}"}}}}"#)),
            Some(CenterTarget::OpenResearch { session })
        );
        assert_eq!(
            read(r#"{"ask":{"prompt":"hi"}}"#),
            Some(CenterTarget::Ask {
                prompt: "hi".into()
            })
        );
    }

    #[test]
    fn target_reads_from_js() {
        let new_chat: CenterTarget =
            serde_json::from_str(r#"{"chat":{"session":null}}"#).expect("a new chat parses");
        assert_eq!(new_chat, CenterTarget::Chat { session: None });
        let memories: CenterTarget = serde_json::from_str(r#""memories""#).expect("parses");
        assert_eq!(memories, CenterTarget::Memories);
        let id = Uuid::new_v4();
        let chat: CenterTarget =
            serde_json::from_str(&format!(r#"{{"chat":{{"session":"{id}"}}}}"#)).expect("parses");
        assert_eq!(chat, CenterTarget::Chat { session: Some(id) });
    }

    #[test]
    fn group_and_agent_targets_read_as_the_pane_takes_them() {
        let group = CenterTarget::Group { id: "g1".into() };
        assert_eq!(
            serde_json::to_value(&group).expect("serializes"),
            serde_json::json!({ "group": { "id": "g1" } })
        );
        let agent = CenterTarget::Agent(AgentTarget::asking("m1", "Studio", "claude", "s1", None));
        let json = serde_json::to_value(&agent).expect("serializes");
        assert_eq!(json["agent"]["kind"], "agent");
        assert_eq!(json["agent"]["id"], "s1");
        assert_eq!(json["agent"]["status"], "asking");
        let back: CenterTarget = serde_json::from_value(json).expect("reads back");
        assert_eq!(back, agent);
    }

    #[test]
    fn pending_is_taken_once() {
        let pending = Pending::default();
        pending.put(CenterTarget::Chats);
        assert_eq!(pending.take(), Some(CenterTarget::Chats));
        assert_eq!(pending.take(), None);
    }
}
