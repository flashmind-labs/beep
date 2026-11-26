//! Handing the main window a view to show: what the bar, the tray, a
//! notice or another window asks it to open. Entry point: `open_on`.

use super::window::{LABEL, open};
use crate::error::Result;
use beep_proto::AgentNoticeKey;
use beep_proto::SessionId;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Runtime, State};

/// A view of the window that the bar, the tray or another window can hand
/// over to.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CenterTarget {
    Home,
    Settings,
    Instructions,
    /// A coding agent's session, as a Running now row names it.
    Agent(Box<AgentTarget>),
    Providers,
    /// The Projects page on `project`, or every project with `None`, as a
    /// project window hands over another project. With `run`, on its Actions
    /// tile with that run open, as a build's notification asks.
    Projects {
        project: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        run: Option<RunAt>,
    },
    Terminals,
    /// Machines in Settings, as a beep-node update's notification asks.
    Machines,
    /// The Report a problem sheet, over the page on screen.
    Report,
}

/// A CI run of a project: its id on `host`, as the Actions tile keys runs.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub(crate) struct RunAt {
    pub(crate) id: u64,
    pub(crate) host: String,
}

/// A coding agent's session to open, shaped as the window's Running now
/// row (`RunningAgent`) so it opens like one.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub(crate) struct AgentTarget {
    kind: String,
    machine_id: String,
    machine_name: String,
    id: SessionId,
    agent: String,
    title: Option<String>,
    cwd: String,
    status: String,
    doing: Option<String>,
    started_at: u64,
    last_activity: u64,
    external: bool,
    /// A temporary reviewer: it opens beside the diff it reviewed.
    #[serde(default)]
    reviewer: bool,
}

impl AgentTarget {
    /// Agent `agent`'s session `session` on machine `machine_id`, which
    /// waits on a question, boxed as `CenterTarget::Agent` holds it.
    pub(crate) fn asking(
        machine_id: &str,
        machine_name: &str,
        agent: &str,
        session: &SessionId,
        title: Option<&str>,
    ) -> Box<Self> {
        Box::new(Self {
            kind: "agent".into(),
            machine_id: machine_id.to_owned(),
            machine_name: machine_name.to_owned(),
            id: session.clone(),
            agent: agent.to_owned(),
            title: title.map(str::to_owned),
            cwd: String::new(),
            status: "asking".into(),
            doing: None,
            started_at: 0,
            last_activity: 0,
            external: false,
            reviewer: false,
        })
    }

    /// The same session, a temporary reviewer.
    pub(crate) fn reviewing(mut self: Box<Self>) -> Box<Self> {
        self.reviewer = true;
        self
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
        *crate::store::lock(&self.0) = Some(target);
    }

    fn take(&self) -> Option<CenterTarget> {
        crate::store::lock(&self.0).take()
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
#[path = "../tests/center_targets.rs"]
mod tests;
