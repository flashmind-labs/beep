//! A window's read-only session subscriptions: the session on screen and
//! those it keeps following off screen, each cancelled when the view drops it.

use super::sessions::{ModesRow, SessionPage, TaskRow};
use crate::{
    center::AgentsShown,
    error::{DesktopError, Result},
    fleet::Machines,
};
use beep_mesh::machine;
use beep_proto::{
    AgentMode, AgentSessionModes, AgentSessionView, AgentSlug, SessionId,
    node::agent::{
        FollowSession, SESSION_FOLLOW_PROTOCOL, SessionCursor, SessionSource, SessionUpdate,
    },
};
use serde::Serialize;
use std::{
    collections::{HashMap, VecDeque},
    sync::{Mutex, MutexGuard, PoisonError},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager, State, WebviewWindow, ipc::Channel};
use tokio::{sync::watch, time::timeout};
use uuid::Uuid;

/// The most readers one window keeps; past it the oldest is cancelled, so a
/// reloaded page cannot leave readers behind without bound.
const WINDOW_READERS: usize = 16;

/// The session a reader follows, marked read while it shows.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Followed {
    machine_id: String,
    agent: String,
    session: String,
}

struct Reader {
    id: Uuid,
    /// Dropping it cancels the reader.
    _stop: watch::Sender<bool>,
    followed: Followed,
}

#[derive(Default)]
struct Readers {
    /// Each window's readers, oldest first.
    active: HashMap<String, Vec<Reader>>,
    /// The reader whose session each window shows.
    shown: HashMap<String, Uuid>,
    cancelled: VecDeque<(String, Uuid)>,
}

impl Readers {
    fn register(&mut self, window: &str, reader: Reader) -> bool {
        if let Some(index) = self
            .cancelled
            .iter()
            .position(|(label, old)| label == window && *old == reader.id)
        {
            self.cancelled.remove(index);
            return false;
        }

        let readers = self.active.entry(window.to_owned()).or_default();
        readers.push(reader);

        if readers.len() > WINDOW_READERS {
            readers.remove(0);
        }

        true
    }

    fn cancel(&mut self, window: &str, id: Uuid) {
        if self.shown.get(window) == Some(&id) {
            self.shown.remove(window);
        }

        if !self.remove(window, id)
            && !self
                .cancelled
                .iter()
                .any(|(label, old)| label == window && *old == id)
        {
            if self.cancelled.len() == 256 {
                self.cancelled.pop_front();
            }

            self.cancelled.push_back((window.to_owned(), id));
        }
    }

    /// Drops reader `id` of `window`, which cancels it; false if it was not there.
    fn remove(&mut self, window: &str, id: Uuid) -> bool {
        let Some(readers) = self.active.get_mut(window) else {
            return false;
        };
        let before = readers.len();
        readers.retain(|reader| reader.id != id);
        let removed = readers.len() != before;

        if readers.is_empty() {
            self.active.remove(window);
        }

        removed
    }

    /// `window` shows reader `id`'s session, or none; the session shown, if
    /// that reader is registered.
    fn show(&mut self, window: &str, id: Option<Uuid>) -> Option<Followed> {
        let id = match id {
            Some(id) => id,
            None => {
                self.shown.remove(window);
                return None;
            }
        };
        self.shown.insert(window.to_owned(), id);
        self.active
            .get(window)?
            .iter()
            .find(|reader| reader.id == id)
            .map(|reader| reader.followed.clone())
    }

    fn shows(&self, window: &str, id: Uuid) -> bool {
        self.shown.get(window) == Some(&id)
    }
}

#[derive(Default)]
pub(crate) struct Follows(Mutex<Readers>);

impl Follows {
    fn readers(&self) -> MutexGuard<'_, Readers> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn clear(&self, window: &str) {
        let mut readers = self.readers();
        readers.active.remove(window);
        readers.shown.remove(window);
    }

    fn remove(&self, window: &str, id: Uuid) {
        self.readers().remove(window, id);
    }

    fn shows(&self, window: &str, id: Uuid) -> bool {
        self.readers().shows(window, id)
    }
}

struct Reading<'a> {
    follows: &'a Follows,
    window: String,
    id: Uuid,
}

impl Drop for Reading<'_> {
    fn drop(&mut self) {
        self.follows.remove(&self.window, self.id);
    }
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum UpdateKind {
    Snapshot,
    Delta,
}

#[derive(Serialize)]
pub struct Update {
    kind: UpdateKind,
    cursor: SessionCursor,
    resumed: bool,
    page: SessionPage,
    modes: Option<ModesRow>,
    tasks: Vec<TaskRow>,
}

#[tauri::command]
pub async fn agent_unfollow(
    window: WebviewWindow,
    follows: State<'_, Follows>,
    id: Uuid,
) -> Result<()> {
    follows.readers().cancel(window.label(), id);
    Ok(())
}

/// The window shows reader `id`'s session, or none: only that one is marked
/// read and keeps its notices quiet. Synchronous, so calls apply in order.
#[tauri::command]
pub fn agent_follow_show(
    app: AppHandle,
    window: WebviewWindow,
    follows: State<'_, Follows>,
    id: Option<Uuid>,
) {
    let Some(followed) = follows.readers().show(window.label(), id) else {
        return;
    };
    let Followed {
        machine_id,
        agent,
        session,
    } = &followed;
    app.state::<AgentsShown>()
        .read(window.label(), machine_id, agent, session, Instant::now());
    crate::notices::history::agent_viewed(&app, machine_id, agent, session);
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn agent_follow(
    app: AppHandle,
    window: WebviewWindow,
    fleet: Machines<'_>,
    follows: State<'_, Follows>,
    id: Uuid,
    machine_id: String,
    agent: String,
    session: String,
    last: u32,
    after: Option<SessionCursor>,
    on_event: Channel<Update>,
) -> Result<()> {
    let (cancel, mut cancelled) = watch::channel(false);

    if app.get_webview_window(window.label()).is_none() {
        return Ok(());
    }

    let reader = Reader {
        id,
        _stop: cancel,
        followed: Followed {
            machine_id: machine_id.clone(),
            agent: agent.clone(),
            session: session.clone(),
        },
    };
    let registered = follows.readers().register(window.label(), reader);
    if !registered {
        return Ok(());
    }

    let _reading = Reading {
        follows: &follows,
        window: window.label().to_owned(),
        id,
    };

    if app.get_webview_window(window.label()).is_none() {
        return Ok(());
    }

    let follow = async {
        let machine = fleet
            .member(&machine_id)
            .ok_or_else(|| DesktopError::BadRequest("machine is not in the mesh".into()))?;
        let client = fleet.client(machine).await?;
        let request = FollowSession {
            agent: Into::<AgentSlug>::into(agent),
            session: Into::<SessionId>::into(session.clone()),
            last,
            after,
        };
        let (_tx, mut rx) = client
            .open_sending(SESSION_FOLLOW_PROTOCOL, &request)
            .await?;
        loop {
            // Heartbeats detect a half-open link even when the session is idle.
            let update = timeout(Duration::from_secs(35), rx.recv::<SessionUpdate>())
                .await
                .map_err(|_| DesktopError::BadRequest("session stream stopped responding".into()))??
                .ok_or_else(|| DesktopError::BadRequest("session stream disconnected".into()))?;
            if app.get_webview_window(window.label()).is_none() {
                return Ok(());
            }

            // Only the session on screen counts as read; those followed off
            // screen still notify.
            let shown = follows.shows(window.label(), id);

            if shown {
                app.state::<AgentsShown>().read(
                    window.label(),
                    &machine_id,
                    request.agent.as_ref(),
                    &session,
                    Instant::now(),
                );
            }

            let (kind, snapshot) = match update {
                SessionUpdate::Snapshot(snapshot) => (UpdateKind::Snapshot, *snapshot),
                SessionUpdate::Delta(snapshot) => (UpdateKind::Delta, *snapshot),
                SessionUpdate::KeepAlive => continue,
                SessionUpdate::Error { reason } => return Err(DesktopError::BadRequest(reason)),
            };
            let (mut view, modes, tasks) = match snapshot.source {
                SessionSource::Owned(info) => {
                    let modes = AgentSessionModes {
                        current: info.mode.clone(),
                        modes: info
                            .modes
                            .iter()
                            .map(|mode| AgentMode {
                                id: mode.id.clone(),
                                name: mode.name.clone(),
                            })
                            .collect(),
                        plan: info.plan_ready.clone(),
                    };
                    let tasks = machine::session_tasks(info.state.tasks.clone()).tasks;
                    (
                        machine::beep_view(&fleet.host_of(machine), *info),
                        Some(modes.into()),
                        tasks,
                    )
                }
                SessionSource::External(info) => {
                    let last_message = info.last_message.clone();
                    (
                        AgentSessionView {
                            model: None,
                            session: machine::external_session(&fleet.host_of(machine), *info),
                            plan: Vec::new(),
                            files: Vec::new(),
                            last_message,
                            error: None,
                            queued: 0,
                            queued_messages: Vec::new(),
                            turns: 0,
                            entries: Vec::new(),
                            earlier: 0,
                        },
                        None,
                        Vec::new(),
                    )
                }
            };
            view.entries = machine::entries_for(snapshot.entries, true);
            view.earlier = u32::try_from(snapshot.earlier).unwrap_or(u32::MAX);

            if let Some(mesh) = fleet.mesh() {
                beep_fleet::session_titles::apply(&mesh, &mut view.session);
            }

            on_event.send(Update {
                kind,
                cursor: snapshot.cursor,
                resumed: snapshot.resumed,
                page: view.into(),
                modes,
                tasks: tasks.into_iter().map(Into::into).collect(),
            })?;
            if shown {
                crate::notices::history::agent_viewed(
                    &app,
                    &machine_id,
                    request.agent.as_ref(),
                    &session,
                );
            }
        }
    };
    tokio::select! {
        result = follow => result,
        _ = cancelled.changed() => Ok(()),
    }
}

#[cfg(test)]
#[path = "../tests/agents_follow.rs"]
mod tests;
