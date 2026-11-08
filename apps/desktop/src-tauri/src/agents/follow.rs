//! One read-only subscription per window, cancelled when the view leaves.

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
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager, State, WebviewWindow, ipc::Channel};
use tokio::{sync::watch, time::timeout};
use uuid::Uuid;

#[derive(Default)]
struct Readers {
    active: HashMap<String, (Uuid, watch::Sender<bool>)>,
    cancelled: VecDeque<(String, Uuid)>,
}

impl Readers {
    fn register(&mut self, window: &str, id: Uuid, stop: watch::Sender<bool>) -> bool {
        if let Some(index) = self
            .cancelled
            .iter()
            .position(|(label, old)| label == window && *old == id)
        {
            self.cancelled.remove(index);
            return false;
        }
        self.active.insert(window.to_owned(), (id, stop));
        true
    }

    fn cancel(&mut self, window: &str, id: Uuid) {
        if self
            .active
            .get(window)
            .is_some_and(|(current, _)| *current == id)
        {
            self.active.remove(window);
        } else if !self
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
}

#[derive(Default)]
pub(crate) struct Follows(Mutex<Readers>);

impl Follows {
    pub(crate) fn clear(&self, window: &str) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .active
            .remove(window);
    }

    fn remove(&self, window: &str, id: Uuid) {
        let mut readers = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if readers
            .active
            .get(window)
            .is_some_and(|(current, _)| *current == id)
        {
            readers.active.remove(window);
        }
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
    follows
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .cancel(window.label(), id);
    Ok(())
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
    let registered = follows
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .register(window.label(), id, cancel);
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
            app.state::<AgentsShown>().read(
                window.label(),
                &machine_id,
                request.agent.as_ref(),
                &session,
                Instant::now(),
            );
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
            crate::notices::history::agent_viewed(
                &app,
                &machine_id,
                request.agent.as_ref(),
                &session,
            );
        }
    };
    tokio::select! {
        result = follow => result,
        _ = cancelled.changed() => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_replaced_reader_and_its_late_detach_cannot_cancel_the_current_reader() {
        let follows = Follows::default();
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        let (old, old_cancelled) = watch::channel(false);
        follows
            .0
            .lock()
            .unwrap()
            .active
            .insert("window".into(), (first, old));
        let reading = Reading {
            follows: &follows,
            window: "window".into(),
            id: first,
        };
        let (new, new_cancelled) = watch::channel(false);
        follows
            .0
            .lock()
            .unwrap()
            .active
            .insert("window".into(), (second, new));
        assert!(old_cancelled.has_changed().is_err());
        drop(reading);
        follows.remove("window", first);
        assert!(new_cancelled.has_changed().is_ok());
        follows.clear("window");
        assert!(new_cancelled.has_changed().is_err());
    }

    #[test]
    fn detaching_before_registration_does_not_leave_an_orphan_reader() {
        let mut readers = Readers::default();
        let id = Uuid::new_v4();
        readers.cancel("window", id);
        let (sender, receiver) = watch::channel(false);
        assert!(!readers.register("window", id, sender));
        assert!(readers.active.is_empty());
        assert!(receiver.has_changed().is_err());
    }

    #[test]
    fn detaching_one_window_keeps_the_other_window_subscribed() {
        let follows = Follows::default();
        let id = Uuid::new_v4();
        let (a, a_cancelled) = watch::channel(false);
        let (b, b_cancelled) = watch::channel(false);
        follows
            .0
            .lock()
            .unwrap()
            .active
            .extend([("a".into(), (id, a)), ("b".into(), (id, b))]);
        follows.remove("a", id);
        assert!(a_cancelled.has_changed().is_err());
        assert!(b_cancelled.has_changed().is_ok());
    }
}
