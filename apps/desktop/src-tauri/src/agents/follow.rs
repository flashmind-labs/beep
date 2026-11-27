//! A window's read-only session subscriptions: the session on screen and
//! those it keeps following off screen, each cancelled when the view drops it.

use super::{
    kept_transcripts::{KeptTranscripts, SessionKey},
    sessions::{ModesRow, SessionPage},
    shown::AgentsShown,
};
use crate::{
    center::window::{OnScreen, hidden},
    error::{DesktopError, Result},
    fleet::Machines,
};
use beep_mesh::machine;
use beep_proto::watched;
use beep_proto::{
    AgentMode, AgentSessionModes, AgentSessionView, AgentSlug, AgentStatus, AgentTask, SessionId,
    node::agent::{
        Entry, EntryPatch, FOLLOW_READERS_FULL, FollowControl, FollowSession,
        SESSION_FOLLOW_PROTOCOL, SessionCursor, SessionPatch, SessionSnapshot, SessionSource,
        SessionUpdate,
    },
};
use serde::Serialize;
use std::future::Future;
use std::pin::Pin;
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager, State, WebviewWindow, ipc::Channel};
use tokio::{
    sync::watch,
    time::{Instant as TokioInstant, Sleep, sleep, sleep_until},
};
use uuid::Uuid;

mod keep;
mod pace;
use keep::Keeper;
use pace::{Pace, Pushed, UpdateKind};

/// The most readers one window keeps; past it the oldest is cancelled, so a
/// reloaded page cannot leave readers behind without bound.
const WINDOW_READERS: usize = 16;

/// The node sends a heartbeat every 15 s, so a stream with no frame in this
/// long is dead.
const SILENT_AFTER: Duration = Duration::from_secs(35);
/// How long a released reader gets to close its stream before the shown
/// session asks the node again.
const RELEASE_WAIT: Duration = Duration::from_millis(500);

/// Whether the node turned a follow away because it has as many session
/// readers as it allows.
fn node_full(reason: &str) -> bool {
    reason == FOLLOW_READERS_FULL
}

struct Reader {
    id: Uuid,
    /// Dropping it cancels the reader.
    _stop: watch::Sender<bool>,
    /// Whether the window shows its session.
    shown: watch::Sender<bool>,
    /// Whether its session asks a question, from its newest update.
    asking: Arc<AtomicBool>,
    /// The session it follows, marked read while it shows.
    followed: SessionKey,
}

impl Reader {
    fn asking(&self) -> bool {
        self.asking.load(Ordering::Relaxed)
    }
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

        reader
            .shown
            .send_replace(self.shown.get(window) == Some(&reader.id));
        let readers = self.active.entry(window.to_owned()).or_default();
        readers.push(reader);

        if readers.len() > WINDOW_READERS {
            let shown = self.shown.get(window);
            let background = |reader: &Reader| Some(&reader.id) != shown;
            let oldest = readers
                .iter()
                .position(|reader| background(reader) && !reader.asking())
                .or_else(|| readers.iter().position(background));

            if let Some(index) = oldest {
                readers.remove(index);
            }
        }

        true
    }

    /// Cancels `window`'s oldest reader on `machine_id` that is neither shown
    /// nor asking, other than `except`, to free a node reader; false if none.
    fn release_on(&mut self, window: &str, machine_id: &str, except: Uuid) -> bool {
        let shown = self.shown.get(window).copied();
        let Some(readers) = self.active.get(window) else {
            return false;
        };
        let Some(id) = readers
            .iter()
            .find(|reader| {
                reader.id != except
                    && Some(reader.id) != shown
                    && reader.followed.machine_id == machine_id
                    && !reader.asking()
            })
            .map(|reader| reader.id)
        else {
            return false;
        };

        self.remove(window, id)
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
    fn show(&mut self, window: &str, id: Option<Uuid>) -> Option<SessionKey> {
        match id {
            Some(id) => self.shown.insert(window.to_owned(), id),
            None => self.shown.remove(window),
        };
        let readers = self.active.get(window)?;

        for reader in readers {
            reader.shown.send_if_modified(|shown| {
                let was = *shown;
                *shown = Some(reader.id) == id;
                was != *shown
            });
        }

        let id = id?;
        readers
            .iter()
            .find(|reader| reader.id == id)
            .map(|reader| reader.followed.clone())
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
pub struct Update {
    kind: UpdateKind,
    cursor: SessionCursor,
    resumed: bool,
    page: SessionPage,
    modes: Option<ModesRow>,
    tasks: Vec<AgentTask>,
}

impl From<Pushed> for Update {
    fn from(pushed: Pushed) -> Self {
        Self {
            kind: pushed.kind,
            cursor: pushed.cursor,
            resumed: pushed.resumed,
            page: pushed.view.into(),
            modes: pushed.modes,
            tasks: pushed.tasks,
        }
    }
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
    let SessionKey {
        machine_id,
        agent,
        session,
    } = &followed;
    app.state::<AgentsShown>()
        .read(window.label(), machine_id, agent, session, Instant::now());
    crate::notes::agent_viewed(&app, machine_id, agent, session);
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
#[tracing::instrument(skip_all, fields(reader = %id, peer = %machine_id, %session))]
pub async fn agent_follow(
    app: AppHandle,
    window: WebviewWindow,
    fleet: Machines<'_>,
    follows: State<'_, Follows>,
    id: Uuid,
    machine_id: String,
    agent: String,
    session: SessionId,
    last: u32,
    after: Option<SessionCursor>,
    on_event: Channel<Update>,
) -> Result<()> {
    crate::agents::starts::not_followed(&session)?;
    let (cancel, mut cancelled) = watch::channel(false);
    let (shown, mut on_screen) = watch::channel(false);
    let asking = Arc::new(AtomicBool::new(false));

    if app.get_webview_window(window.label()).is_none() {
        return Ok(());
    }

    let reader = Reader {
        id,
        _stop: cancel,
        shown,
        asking: asking.clone(),
        followed: SessionKey {
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

    let started = Instant::now();
    let follow = async {
        let machine = fleet
            .member(&machine_id)
            .ok_or_else(|| DesktopError::BadRequest("machine is not in the mesh".into()))?;
        let client = fleet.client(machine).await?;
        // A hidden window hears nothing until it shows again.
        let mut window_shown = app.state::<OnScreen>().watch(window.label());
        let initial_shown = watched::read(&on_screen, |shown| *shown) && !hidden(&window_shown);
        let request = FollowSession {
            agent: Into::<AgentSlug>::into(agent),
            session: Into::<SessionId>::into(session.clone()),
            last,
            after,
            patches: true,
            shown: initial_shown,
        };
        let (mut tx, mut rx) = client
            .open_sending(SESSION_FOLLOW_PROTOCOL, &request)
            .await?;
        let mut pace = Pace::default();
        let mut follow_state = FollowState::default();
        let mut node_controls = false;
        let mut sent_shown = initial_shown;
        let followed = SessionKey {
            machine_id: machine_id.clone(),
            agent: request.agent.as_ref().to_owned(),
            session: session.clone(),
        };
        let mut keeper = Keeper::new(
            app.state::<Arc<KeptTranscripts>>().inner().clone(),
            followed.clone(),
        );
        let mut retried = false;
        let mut waiting_for_page = Some(started);
        let mut watching = true;
        pace.window(hidden(&window_shown), Instant::now());

        let mut silence = Silence::new();

        loop {
            let due = pace.due();
            let keep_due = keeper.due();
            let update = match wake(
                rx.recv::<SessionUpdate>(),
                &mut silence,
                due,
                keep_due,
                &mut on_screen,
                &mut window_shown,
                watching,
            )
            .await?
            {
                Woke::Frame(update) => update,
                Woke::Pace => {
                    if let Some(pushed) = pace.flush(Instant::now()) {
                        on_event.send(pushed.into())?;
                    }

                    continue;
                }
                Woke::Keep => {
                    keeper.write(Instant::now());
                    continue;
                }
                Woke::Screen { closed } => {
                    if closed {
                        return Ok(());
                    }

                    let shown_now = watched::read_and_update(&mut on_screen, |shown| *shown);
                    let visible = shown_now && !hidden(&window_shown);

                    if node_controls && visible != sent_shown {
                        sent_shown = visible;
                        let _ = tx.send(&FollowControl::Shown(visible)).await;
                    }

                    // Shown again: what was held goes at once.
                    if shown_now && let Some(pushed) = pace.flush(Instant::now()) {
                        on_event.send(pushed.into())?;
                    }

                    continue;
                }
                Woke::Window { closed } => {
                    // The window is gone: the next update ends the follow.
                    let hidden = !closed && hidden(&window_shown);
                    watching = !closed;
                    let visible = !hidden && watched::read(&on_screen, |shown| *shown);

                    if node_controls && visible != sent_shown {
                        sent_shown = visible;
                        let _ = tx.send(&FollowControl::Shown(visible)).await;
                    }

                    if hidden {
                        keeper.write(Instant::now());
                    }

                    if let Some(pushed) = pace.window(hidden, Instant::now()) {
                        on_event.send(pushed.into())?;
                    }

                    continue;
                }
            };
            if app.get_webview_window(window.label()).is_none() {
                return Ok(());
            }

            // Only the session on screen counts as read; those followed off
            // screen still notify.
            let shown = watched::read(&on_screen, |shown| *shown);

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
                SessionUpdate::Snapshot(snapshot) => {
                    let snapshot = *snapshot;
                    follow_state.record_snapshot(&snapshot, true);
                    if snapshot.controls {
                        node_controls = true;
                    }
                    (UpdateKind::Snapshot, snapshot)
                }
                SessionUpdate::Delta(snapshot) => {
                    let snapshot = *snapshot;
                    follow_state.record_snapshot(&snapshot, false);
                    if snapshot.controls {
                        node_controls = true;
                    }
                    (UpdateKind::Delta, snapshot)
                }
                SessionUpdate::Patch(patch) => {
                    node_controls = true;
                    let snapshot = follow_state.apply_patch(*patch)?;
                    (UpdateKind::Delta, snapshot)
                }
                SessionUpdate::KeepAlive => continue,
                // A full node: the shown session takes the place of one of
                // this window's background readers there, once. A background
                // reader turned away just ends; it reads again when opened.
                SessionUpdate::Error { reason }
                    if shown
                        && !retried
                        && node_full(&reason)
                        && follows
                            .readers()
                            .release_on(window.label(), &machine_id, id) =>
                {
                    retried = true;
                    waiting_for_page = Some(Instant::now());
                    sleep(RELEASE_WAIT).await;
                    let visible =
                        watched::read(&on_screen, |shown| *shown) && !hidden(&window_shown);
                    let mut retry_request = request.clone();
                    retry_request.shown = visible;
                    (tx, rx) = client
                        .open_sending(SESSION_FOLLOW_PROTOCOL, &retry_request)
                        .await?;
                    follow_state = FollowState::default();
                    node_controls = false;
                    sent_shown = visible;
                    silence.heard();
                    continue;
                }
                SessionUpdate::Error { reason } => return Err(DesktopError::BadRequest(reason)),
            };

            let visible = shown && !hidden(&window_shown);
            if node_controls && visible != sent_shown {
                sent_shown = visible;
                let _ = tx.send(&FollowControl::Shown(visible)).await;
            }
            if let Some(started) = waiting_for_page.take() {
                tracing::info!(
                    elapsed_ms = started.elapsed().as_millis(),
                    resumed = snapshot.resumed,
                    "session follow received its first page"
                );
            }

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
                            last_stop: None,
                            usage: None,
                        },
                        None,
                        Vec::new(),
                    )
                }
            };
            asking.store(
                view.session.question.is_some()
                    || matches!(view.session.status, AgentStatus::Asking {}),
                Ordering::Relaxed,
            );
            view.entries = machine::entries_for(snapshot.entries, true);
            view.earlier = u32::try_from(snapshot.earlier).unwrap_or(u32::MAX);

            // What the outbox showed as sent goes once this push lists it.
            if !view.session.external {
                super::outbox::settled(
                    &app,
                    &followed,
                    &view.queued_messages,
                    snapshot.earlier,
                    &view.entries,
                );
            }

            if let Some(mesh) = fleet.mesh() {
                beep_fleet::session_titles::apply(&mesh, &mut view.session);
            }

            let pushed = Pushed {
                kind,
                cursor: snapshot.cursor,
                resumed: snapshot.resumed,
                view,
                modes,
                tasks,
            };

            keeper.take(&pushed, Instant::now()).await;

            for pushed in pace.push(pushed, shown, Instant::now()) {
                on_event.send(pushed.into())?;
            }

            if shown {
                crate::notes::agent_viewed(&app, &machine_id, request.agent.as_ref(), &session);
            }
        }
    };
    let result = tokio::select! {
        result = follow => result,
        _ = cancelled.changed() => {
            tracing::info!(elapsed_ms = started.elapsed().as_millis(), "session follow cancelled by desktop");
            return Ok(());
        },
    };
    if let Err(error) = &result {
        tracing::info!(elapsed_ms = started.elapsed().as_millis(), %error, "session follow failed");
    }

    result
}

/// What ended one wait of the follow loop.
enum Woke<T> {
    Frame(T),
    Pace,
    Keep,
    Screen { closed: bool },
    Window { closed: bool },
}

/// Waits for the node's next frame or for something else the loop does:
/// held pages due, a transcript write due, a change of what is on screen or
/// of the window. Only a frame postpones `silence`; the node's heartbeats
/// make a quiet but healthy stream send one at least every 15 s.
async fn wake<T, E: Into<DesktopError>>(
    recv: impl Future<Output = std::result::Result<Option<T>, E>>,
    silence: &mut Silence,
    due: Option<Instant>,
    keep_due: Option<Instant>,
    on_screen: &mut watch::Receiver<bool>,
    window_shown: &mut watch::Receiver<Option<bool>>,
    watching: bool,
) -> Result<Woke<T>> {
    tokio::select! {
        frame = recv => {
            silence.heard();
            frame
                .map_err(Into::into)?
                .map(Woke::Frame)
                .ok_or_else(|| DesktopError::BadRequest("session stream disconnected".into()))
        }
        () = silence.expired() => {
            Err(DesktopError::BadRequest("session stream stopped responding".into()))
        }
        () = sleep_until(due.map_or_else(TokioInstant::now, TokioInstant::from_std)), if due.is_some() => {
            Ok(Woke::Pace)
        }
        () = sleep_until(keep_due.map_or_else(TokioInstant::now, TokioInstant::from_std)), if keep_due.is_some() => {
            Ok(Woke::Keep)
        }
        changed = on_screen.changed() => Ok(Woke::Screen { closed: changed.is_err() }),
        changed = window_shown.changed(), if watching => Ok(Woke::Window { closed: changed.is_err() }),
    }
}

/// A deadline for the next frame from the node. Only a frame resets it,
/// never the pace, keeper or window arms of the follow loop.
struct Silence(Pin<Box<Sleep>>);

impl Silence {
    fn new() -> Self {
        Self(Box::pin(sleep(SILENT_AFTER)))
    }

    fn heard(&mut self) {
        self.0.as_mut().reset(TokioInstant::now() + SILENT_AFTER);
    }

    async fn expired(&mut self) {
        self.0.as_mut().await;
    }
}

/// Reconstructs contiguous snapshots from incremental `SessionUpdate::Patch` frames
/// so downstream pacing, disk caching, and webview state stay unchanged.
#[derive(Default)]
struct FollowState {
    source: Option<SessionSource>,
    earlier: u64,
    entries: Vec<Entry>,
}

impl FollowState {
    fn record_snapshot(&mut self, snapshot: &SessionSnapshot, is_snapshot: bool) {
        self.source = Some(snapshot.source.clone());

        if (is_snapshot && !snapshot.resumed)
            || self.entries.is_empty()
            || snapshot.earlier <= self.earlier
        {
            self.earlier = snapshot.earlier;
            self.entries = snapshot.entries.clone();
        } else {
            let end = self.earlier.saturating_add(self.entries.len() as u64);

            if snapshot.earlier <= end
                && let Ok(keep) = usize::try_from(snapshot.earlier - self.earlier)
            {
                self.entries.truncate(keep);
                self.entries.extend(snapshot.entries.iter().cloned());
            } else {
                self.earlier = snapshot.earlier;
                self.entries = snapshot.entries.clone();
            }
        }

        self.trim_to(snapshot.cursor.start);
    }

    fn apply_patch(&mut self, patch: SessionPatch) -> Result<SessionSnapshot> {
        if let Some(source) = patch.source {
            self.source = Some(source);
        }

        let source = self.source.clone().ok_or_else(|| {
            DesktopError::BadRequest("session patch arrived before snapshot".into())
        })?;
        let mut min_touched: Option<u64> = None;

        for op in patch.ops {
            match op {
                EntryPatch::AppendText { index, text } => {
                    let offset = index
                        .checked_sub(self.earlier)
                        .and_then(|d| usize::try_from(d).ok())
                        .ok_or_else(|| {
                            DesktopError::BadRequest("session patch append out of range".into())
                        })?;
                    let entry = self.entries.get_mut(offset).ok_or_else(|| {
                        DesktopError::BadRequest("session patch append target missing".into())
                    })?;
                    entry.text.push_str(&text);
                    min_touched = Some(min_touched.map_or(index, |m| m.min(index)));
                }
                EntryPatch::Replace { index, entry } => {
                    let offset = index
                        .checked_sub(self.earlier)
                        .and_then(|d| usize::try_from(d).ok())
                        .ok_or_else(|| {
                            DesktopError::BadRequest("session patch replace out of range".into())
                        })?;

                    if offset < self.entries.len() {
                        self.entries[offset] = entry;
                    } else if offset == self.entries.len() {
                        self.entries.push(entry);
                    } else {
                        return Err(DesktopError::BadRequest(
                            "session patch replace left a gap".into(),
                        ));
                    }

                    min_touched = Some(min_touched.map_or(index, |m| m.min(index)));
                }
                EntryPatch::Truncate { next } => {
                    let keep = next
                        .checked_sub(self.earlier)
                        .and_then(|d| usize::try_from(d).ok())
                        .ok_or_else(|| {
                            DesktopError::BadRequest("session patch truncate out of range".into())
                        })?;
                    self.entries.truncate(keep);
                    min_touched = Some(min_touched.map_or(next, |m| m.min(next)));
                }
            }
        }

        self.trim_to(patch.cursor.start);
        let earlier = min_touched
            .unwrap_or(patch.cursor.next)
            .max(self.earlier)
            .min(patch.cursor.next);
        let start = usize::try_from(earlier - self.earlier).unwrap_or(self.entries.len());
        let entries = self.entries[start..].to_vec();

        Ok(SessionSnapshot {
            source,
            entries,
            earlier,
            cursor: patch.cursor,
            resumed: false,
            controls: true,
        })
    }

    fn trim_to(&mut self, start: u64) {
        if start > self.earlier {
            let cut = (start - self.earlier).min(self.entries.len() as u64);

            if let Ok(cut_usize) = usize::try_from(cut) {
                self.entries.drain(..cut_usize);
                self.earlier += cut;
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/agents_follow.rs"]
mod tests;
