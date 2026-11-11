//! Messages for a session whose machine cannot be reached, kept on this
//! device until it can. Every message to a mesh session goes through here,
//! so one session's messages leave in the order typed: one waits behind an
//! unsent one even while the link is up. The message the user just sent is
//! answered by its first try; if that could not reach the machine it waits
//! here, and every window shows it in the session's queue. Each session with
//! waiting messages has one flusher, which sends them once the fleet sees the
//! machine online. The outbox is one file in the app data folder, written by
//! its own thread.

use crate::{
    agents::{
        kept_transcripts::SessionKey,
        steer::{PastedImage, Steer, SteerError, sent},
    },
    error::{DesktopError, Result},
    fleet::{Fleet, agents as direct, asked::READ_LIMIT},
};
use beep_node::link::reconnect::Backoff;
use beep_proto::{AgentEntry, AgentEntryKind, ApiError, GetAgentSession};
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::ErrorKind,
    path::PathBuf,
    sync::{Arc, Mutex, PoisonError, mpsc},
    thread,
};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::{sync::oneshot, time::sleep};

/// The event every window hears when a session's outbox changes.
const CHANGED: &str = "outbox-changed";
/// How many entries a check after a dropped send reads.
const CHECK_LAST: u32 = 60;

/// How a message goes to the session: a prompt for its next turn, or a
/// message for the running turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Kind {
    Prompt,
    Steer,
}

/// Where a kept message is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Stage {
    /// Not sent yet: behind another message, or its machine could not be reached.
    Waiting,
    Sending,
    /// The machine refused it; it stays until the user edits or removes it.
    Failed {
        reason: String,
    },
}

/// One message kept for a session.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Message {
    pub(crate) id: u64,
    pub(crate) machine_id: String,
    pub(crate) agent: String,
    pub(crate) session: String,
    pub(crate) how: Kind,
    pub(crate) text: String,
    pub(crate) images: Vec<PastedImage>,
    pub(crate) state: Stage,
    /// A send of it may have reached the machine before the link dropped,
    /// so the session is read before it is sent again.
    pub(crate) unsure: bool,
}

impl Message {
    fn key(&self) -> SessionKey {
        SessionKey {
            machine_id: self.machine_id.clone(),
            agent: self.agent.clone(),
            session: self.session.clone(),
        }
    }

    fn steer(&self) -> Steer {
        let (text, images) = (self.text.clone(), self.images.clone());
        match self.how {
            Kind::Prompt => Steer::Prompt { text, images },
            Kind::Steer => Steer::InTurn { text, images },
        }
    }

    fn row(&self) -> Row {
        let (state, reason) = match &self.state {
            Stage::Waiting => ("waiting", None),
            Stage::Sending => ("sending", None),
            Stage::Failed { reason } => ("failed", Some(reason.clone())),
        };
        Row {
            id: self.id,
            how: self.how,
            text: self.text.clone(),
            images: self.images.iter().map(|image| image.name.clone()).collect(),
            state,
            reason,
        }
    }
}

/// A kept message as the windows show it, without its image data.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Row {
    pub(crate) id: u64,
    pub(crate) how: Kind,
    pub(crate) text: String,
    pub(crate) images: Vec<String>,
    /// "waiting", "sending" or "failed".
    pub(crate) state: &'static str,
    pub(crate) reason: Option<String>,
}

/// What a session's outbox holds now, and the kept messages that went to
/// the machine since it last changed.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Changed {
    pub(crate) machine_id: String,
    pub(crate) agent: String,
    pub(crate) session: String,
    pub(crate) rows: Vec<Row>,
    pub(crate) sent: Vec<Row>,
}

/// The answer to the window that sent a message, after its first try.
#[derive(Debug)]
pub(crate) enum Answer {
    /// The machine took it; the keys of its images.
    Sent(Vec<String>),
    /// It waits here for the machine.
    Waiting,
    /// The machine refused it, and it is no longer kept.
    Refused(String),
}

/// The kept messages, and what is being done with them.
#[derive(Default)]
pub(crate) struct Held {
    messages: Vec<Message>,
    next: u64,
    /// Sessions whose flusher runs.
    flushing: HashSet<SessionKey>,
    /// The windows waiting on a message's first try. A message with one is
    /// not shown in the outbox: the window shows it as sending.
    replies: HashMap<u64, oneshot::Sender<Answer>>,
}

impl Held {
    fn from_messages(mut messages: Vec<Message>) -> Self {
        // A send cut short by quitting may have gone.
        for message in &mut messages {
            if message.state == Stage::Sending {
                message.state = Stage::Waiting;
                message.unsure = true;
            }
        }

        let next = messages.iter().map(|m| m.id).max().map_or(0, |id| id + 1);
        Self {
            messages,
            next,
            ..Self::default()
        }
    }

    /// Keeps a message for `key`; true when its session needs a flusher started.
    pub(crate) fn push(
        &mut self,
        key: &SessionKey,
        how: Kind,
        text: String,
        images: Vec<PastedImage>,
        reply: Option<oneshot::Sender<Answer>>,
    ) -> (u64, bool) {
        let id = self.next;
        self.next += 1;
        self.messages.push(Message {
            id,
            machine_id: key.machine_id.clone(),
            agent: key.agent.clone(),
            session: key.session.clone(),
            how,
            text,
            images,
            state: Stage::Waiting,
            unsure: false,
        });

        if let Some(reply) = reply {
            self.replies.insert(id, reply);
        }

        (id, self.flushing.insert(key.clone()))
    }

    /// The oldest message of `key` to send, marked as sending; `None` ends
    /// its flusher.
    pub(crate) fn begin(&mut self, key: &SessionKey) -> Option<Message> {
        let Some(message) = self
            .messages
            .iter_mut()
            .find(|m| m.key() == *key && m.state == Stage::Waiting)
        else {
            self.flushing.remove(key);
            return None;
        };
        message.state = Stage::Sending;
        Some(message.clone())
    }

    /// The machine took message `id`: it goes.
    pub(crate) fn sent(&mut self, id: u64, keys: Vec<String>) -> Option<Changed> {
        let at = self.messages.iter().position(|m| m.id == id)?;
        let message = self.messages.remove(at);

        if let Some(reply) = self.replies.remove(&id) {
            let _ = reply.send(Answer::Sent(keys));
            return None;
        }

        let mut changed = self.changed(&message.key());
        changed.sent.push(message.row());
        Some(changed)
    }

    /// Message `id` could not reach its machine: it and those after it wait,
    /// and the windows that sent them are told so.
    pub(crate) fn wait(&mut self, id: u64, unsure: bool) -> Option<Changed> {
        let message = self.messages.iter_mut().find(|m| m.id == id)?;
        message.state = Stage::Waiting;
        message.unsure = unsure;
        let key = message.key();
        let ids: Vec<u64> = self
            .messages
            .iter()
            .filter(|m| m.key() == key)
            .map(|m| m.id)
            .collect();

        for id in ids {
            if let Some(reply) = self.replies.remove(&id) {
                let _ = reply.send(Answer::Waiting);
            }
        }

        Some(self.changed(&key))
    }

    /// The machine refused message `id`. The window that just sent it puts
    /// it back in its field; one kept from before stays with the reason.
    pub(crate) fn refused(&mut self, id: u64, reason: String) -> Option<Changed> {
        let at = self.messages.iter().position(|m| m.id == id)?;

        if let Some(reply) = self.replies.remove(&id) {
            self.messages.remove(at);
            let _ = reply.send(Answer::Refused(reason));
            return None;
        }

        self.messages[at].state = Stage::Failed { reason };
        let key = self.messages[at].key();
        Some(self.changed(&key))
    }

    /// Takes message `id` out, unless it is being sent.
    pub(crate) fn take(&mut self, id: u64) -> Result<(Message, Changed)> {
        let at = self
            .messages
            .iter()
            .position(|m| m.id == id)
            .ok_or_else(|| DesktopError::BadRequest("That message is no longer waiting.".into()))?;

        if self.messages[at].state == Stage::Sending {
            return Err(DesktopError::BadRequest(
                "That message is being sent.".into(),
            ));
        }

        let message = self.messages.remove(at);
        self.replies.remove(&id);
        let changed = self.changed(&message.key());
        Ok((message, changed))
    }

    /// The rows the windows show for `key`, in the order typed.
    pub(crate) fn rows(&self, key: &SessionKey) -> Vec<Row> {
        self.messages
            .iter()
            .filter(|m| m.key() == *key && !self.replies.contains_key(&m.id))
            .map(Message::row)
            .collect()
    }

    /// The sessions with messages to send, each once.
    pub(crate) fn waiting(&self) -> Vec<SessionKey> {
        let mut keys: Vec<SessionKey> = Vec::new();

        for message in &self.messages {
            let key = message.key();

            if message.state == Stage::Waiting && !keys.contains(&key) {
                keys.push(key);
            }
        }

        keys
    }

    fn changed(&self, key: &SessionKey) -> Changed {
        Changed {
            machine_id: key.machine_id.clone(),
            agent: key.agent.clone(),
            session: key.session.clone(),
            rows: self.rows(key),
            sent: Vec::new(),
        }
    }
}

/// What the writer thread does.
enum Write {
    Bytes(Vec<u8>),
    /// Answers once what was asked before is on disk.
    Synced(mpsc::Sender<()>),
}

/// The kept messages and the thread that writes them.
pub struct Outbox {
    held: Mutex<Held>,
    writes: mpsc::Sender<Write>,
}

impl Outbox {
    /// The outbox kept in `file`, or an empty one.
    pub fn load(file: PathBuf) -> Self {
        let messages = match fs::read(&file) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                log::warn!("dropping the unreadable {}: {e}", file.display());
                Vec::new()
            }),
            Err(e) if e.kind() == ErrorKind::NotFound => Vec::new(),
            Err(e) => {
                log::warn!("reading {}: {e}", file.display());
                Vec::new()
            }
        };
        let (writes, queue) = mpsc::channel();
        let spawned = thread::Builder::new()
            .name("outbox".into())
            .spawn(move || write_all(&file, &queue));

        if let Err(e) = spawned {
            log::warn!("starting the outbox writer: {e}");
        }

        Self {
            held: Mutex::new(Held::from_messages(messages)),
            writes,
        }
    }

    /// Changes the held messages with `change`, then has them written.
    pub(crate) fn with<T>(&self, change: impl FnOnce(&mut Held) -> T) -> T {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        let out = change(&mut held);

        match serde_json::to_vec(&held.messages) {
            Ok(bytes) => {
                let _ = self.writes.send(Write::Bytes(bytes));
            }
            Err(e) => log::warn!("keeping the outbox: {e}"),
        }

        out
    }

    fn read<T>(&self, read: impl FnOnce(&Held) -> T) -> T {
        read(&self.held.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// Returns once what changed so far is on disk.
    pub fn sync(&self) {
        let (done, wait) = mpsc::channel();

        if self.writes.send(Write::Synced(done)).is_ok() {
            let _ = wait.recv();
        }
    }
}

/// Writes the newest bytes asked for, replacing the file whole.
fn write_all(file: &PathBuf, queue: &mpsc::Receiver<Write>) {
    let mut synced = Vec::new();

    while let Ok(first) = queue.recv() {
        let mut bytes = None;

        for write in std::iter::once(first).chain(queue.try_iter()) {
            match write {
                Write::Bytes(next) => bytes = Some(next),
                Write::Synced(done) => synced.push(done),
            }
        }

        if let Some(bytes) = bytes {
            let mut partial = file.as_os_str().to_owned();
            partial.push(".part");
            let written = file
                .parent()
                .map_or(Ok(()), fs::create_dir_all)
                .and_then(|()| fs::write(&partial, &bytes))
                .and_then(|()| fs::rename(&partial, file));

            if let Err(e) = written {
                log::warn!("writing {}: {e}", file.display());
            }
        }

        for done in synced.drain(..) {
            let _ = done.send(());
        }
    }
}

/// What a flusher needs of the machine and the windows.
pub(crate) trait Link {
    /// Sends `message`.
    async fn send(&mut self, message: &Message) -> Result<Vec<String>>;
    /// Whether the session, read again, shows the machine has `message`.
    async fn took(&mut self, message: &Message) -> Result<bool>;
    /// Returns when the machine may answer again.
    async fn reachable(&mut self);
    /// Tells the windows what the session's outbox holds now.
    fn changed(&mut self, changed: Changed);
}

/// What became of one try at the oldest waiting message.
enum Tried {
    Sent(Vec<String>),
    Lost { unsure: bool },
    Refused(String),
}

/// Sends `key`'s waiting messages oldest first, until none is left. One
/// that could not reach the machine waits for it to be reachable, and
/// those after it wait behind it. One the machine refused keeps its reason
/// and the next goes.
pub(crate) async fn flush(outbox: &Outbox, key: &SessionKey, link: &mut impl Link) {
    while let Some(message) = outbox.with(|held| held.begin(key)) {
        let id = message.id;
        let tried = try_once(link, &message).await;
        let lost = matches!(tried, Tried::Lost { .. });
        let change = outbox.with(|held| match tried {
            Tried::Sent(keys) => held.sent(id, keys),
            Tried::Lost { unsure } => held.wait(id, unsure),
            Tried::Refused(reason) => held.refused(id, reason),
        });

        if let Some(change) = change {
            link.changed(change);
        }

        if lost {
            link.reachable().await;
        }
    }
}

/// One try at `message`. One whose last send may have gone is sent again
/// only once a read of the session shows the machine does not have it.
async fn try_once(link: &mut impl Link, message: &Message) -> Tried {
    if message.unsure {
        match link.took(message).await {
            Ok(true) => return Tried::Sent(Vec::new()),
            Ok(false) => {}
            Err(e) if e.is_transport() => return Tried::Lost { unsure: true },
            Err(DesktopError::Api(ApiError::NotFound {})) => {
                return Tried::Refused("This session is no longer on the machine.".into());
            }
            Err(e) => return Tried::Refused(e.to_string()),
        }
    }

    match link.send(message).await.map_err(SteerError::from) {
        Ok(keys) => Tried::Sent(keys),
        Err(SteerError::Lost { .. }) => Tried::Lost { unsure: false },
        Err(SteerError::Unsure { .. }) => Tried::Lost { unsure: true },
        Err(SteerError::Failed { message } | SteerError::Waiting { message }) => {
            Tried::Refused(message)
        }
    }
}

/// A session's machine, as the flusher reaches it.
struct MeshLink {
    app: AppHandle,
    /// The window that started the flusher, whose event stream may carry the call.
    window: String,
    machine: EndpointId,
    backoff: Backoff,
}

impl Link for MeshLink {
    async fn send(&mut self, message: &Message) -> Result<Vec<String>> {
        let app = self.app.clone();
        let keys = sent(
            app.clone(),
            &self.window,
            app.state(),
            app.state(),
            message.machine_id.clone(),
            message.agent.clone(),
            message.session.clone(),
            message.steer(),
        )
        .await?;
        self.backoff.reset();
        Ok(keys)
    }

    async fn took(&mut self, message: &Message) -> Result<bool> {
        let fleet = self.app.state::<Arc<Fleet>>();
        let request = GetAgentSession {
            machine_id: message.machine_id.clone(),
            agent: message.agent.clone(),
            session: message.session.clone(),
            last: CHECK_LAST,
            before: None,
        };
        let read = direct::session(&fleet, self.machine, &request);
        let view = fleet
            .asked(
                self.machine,
                &message.session,
                "checking a dropped send",
                READ_LIMIT,
                read,
            )
            .await?;
        Ok(holds(&view.queued_messages, &view.entries, &message.text))
    }

    async fn reachable(&mut self) {
        let fleet = self.app.state::<Arc<Fleet>>().inner().clone();
        crate::fleet::alerts::until_online(&fleet, self.machine).await;
        sleep(self.backoff.pause()).await;
    }

    fn changed(&mut self, changed: Changed) {
        emit(&self.app, changed);
    }
}

/// Whether a session, read after a send that may have gone, shows the
/// machine took `text`: it waits in the session's `queued` messages, or the
/// user's last message in `entries` is it.
fn holds(queued: &[String], entries: &[AgentEntry], text: &str) -> bool {
    let text = text.trim();
    let last = entries
        .iter()
        .rev()
        .find(|e| matches!(e.kind, AgentEntryKind::User {}));
    !text.is_empty()
        && (queued.iter().any(|m| m.contains(text)) || last.is_some_and(|e| e.text.contains(text)))
}

fn emit(app: &AppHandle, changed: Changed) {
    let _ = app.emit(CHANGED, changed);
}

/// Runs `key`'s flusher, which the caller marked as flushing.
fn start(app: &AppHandle, key: SessionKey, window: String) {
    let Some(machine) = app.state::<Arc<Fleet>>().member(&key.machine_id) else {
        // No longer in the mesh: what waits stays, shown, until removed.
        app.state::<Arc<Outbox>>()
            .with(|held| held.flushing.remove(&key));
        return;
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let outbox = app.state::<Arc<Outbox>>().inner().clone();
        let mut link = MeshLink {
            app: app.clone(),
            window,
            machine,
            backoff: Backoff::eager(),
        };
        flush(&outbox, &key, &mut link).await;
    });
}

/// Starts a flusher for each session with messages kept from the last run.
pub fn resume(app: &AppHandle) {
    let outbox = app.state::<Arc<Outbox>>();
    let keys = outbox.with(|held| {
        let keys = held.waiting();
        held.flushing.extend(keys.iter().cloned());
        keys
    });

    for key in keys {
        start(app, key, String::new());
    }
}

/// Sends a message to `key`'s session through the outbox, after the ones
/// waiting there: the machine's answer to its first try, or `Waiting`.
pub(crate) async fn send(
    app: &AppHandle,
    window: &str,
    key: SessionKey,
    steer: Steer,
) -> std::result::Result<Vec<String>, SteerError> {
    let (how, text, images) = match steer {
        Steer::Prompt { text, images } => (Kind::Prompt, text, images),
        Steer::InTurn { text, images } => (Kind::Steer, text, images),
        _ => {
            return Err(SteerError::Failed {
                message: "only messages wait for their machine".into(),
            });
        }
    };
    let (reply, answer) = oneshot::channel();
    let outbox = app.state::<Arc<Outbox>>();
    let (_, flusher) = outbox.with(|held| held.push(&key, how, text, images, Some(reply)));

    if flusher {
        start(app, key, window.to_string());
    }

    match answer.await {
        Ok(Answer::Sent(keys)) => Ok(keys),
        Ok(Answer::Refused(message)) => Err(SteerError::Failed { message }),
        Ok(Answer::Waiting) | Err(_) => Err(SteerError::Waiting {
            message: "Waiting for the connection".into(),
        }),
    }
}

/// The messages waiting on this device for a session.
#[tauri::command]
pub fn outbox_list(
    outbox: State<'_, Arc<Outbox>>,
    machine_id: String,
    agent: String,
    session: String,
) -> Vec<Row> {
    let key = SessionKey {
        machine_id,
        agent,
        session,
    };
    outbox.read(|held| held.rows(&key))
}

/// A waiting message as the field takes it back.
#[derive(Debug, Serialize)]
pub struct Taken {
    text: String,
    images: Vec<PastedImage>,
}

/// Takes a waiting message out of the outbox, to remove or edit it.
#[tauri::command]
pub fn outbox_take(app: AppHandle, outbox: State<'_, Arc<Outbox>>, id: u64) -> Result<Taken> {
    let (message, changed) = outbox.with(|held| held.take(id))?;
    emit(&app, changed);
    Ok(Taken {
        text: message.text,
        images: message.images,
    })
}

#[cfg(test)]
#[path = "../tests/agents_outbox.rs"]
mod tests;
