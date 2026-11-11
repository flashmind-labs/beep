//! Messages for a session whose machine cannot be reached, kept on this
//! device until it can. Every message to a mesh session goes through here,
//! so one session's messages leave in the order typed: one waits behind an
//! unsent one even while the link is up. The message the user just sent is
//! answered by its first try, or at once when older ones wait for the
//! machine; one that waits is shown in the session's queue in every window.
//! Each session with waiting messages has one flusher, which sends them once
//! the fleet sees the machine online. The outbox is a folder in the app data
//! folder: the messages in one file, each image in its own, written by the
//! outbox's own thread.

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
    fs::{self, File},
    io::{ErrorKind, Write as _},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError, mpsc},
    thread,
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::{
    sync::oneshot,
    time::{sleep, timeout},
};

/// The event every window hears when a session's outbox changes.
const CHANGED: &str = "outbox-changed";
/// How many entries a check after a dropped send reads.
const CHECK_LAST: u32 = 60;
/// How long the window that sent a message waits for its first try, within
/// the page's own 40 s; after that it waits in the outbox, shown there.
const ANSWER_WAIT: Duration = Duration::from_secs(30);

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

/// What the window saw of the session when it sent a message, so a check
/// after a dropped send counts only what came after.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Seen {
    /// Entries in the session's log: those before the page and on it.
    pub(crate) entries: u64,
    /// The messages the session held for its turn.
    pub(crate) queued: Vec<String>,
}

/// An image of a kept message. Its bytes are kept in a file of their own,
/// not in the messages' file.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct KeptImage {
    pub(crate) name: String,
    pub(crate) mime: String,
    #[serde(skip)]
    pub(crate) data: String,
}

impl From<PastedImage> for KeptImage {
    fn from(image: PastedImage) -> Self {
        Self {
            name: image.name,
            mime: image.mime,
            data: image.data,
        }
    }
}

impl From<KeptImage> for PastedImage {
    fn from(image: KeptImage) -> Self {
        Self {
            name: image.name,
            mime: image.mime,
            data: image.data,
        }
    }
}

/// A message as a window hands it to the outbox.
pub(crate) struct Draft {
    pub(crate) how: Kind,
    pub(crate) text: String,
    pub(crate) images: Vec<PastedImage>,
    pub(crate) seen: Option<Seen>,
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
    pub(crate) images: Vec<KeptImage>,
    pub(crate) state: Stage,
    /// A send of it may have reached the machine before the link dropped,
    /// so the session is read before it is sent again.
    pub(crate) unsure: bool,
    #[serde(default)]
    pub(crate) seen: Option<Seen>,
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
        let text = self.text.clone();
        let images = self.images.iter().cloned().map(Into::into).collect();
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

    fn taken(self) -> Taken {
        Taken {
            text: self.text,
            images: self.images.into_iter().map(Into::into).collect(),
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

/// A change to the outbox's folder, done in order by its thread.
enum Write {
    /// The messages' file, as it is now.
    Messages(Vec<u8>),
    /// An image of a message, kept until it goes.
    Image { file: String, data: String },
    /// The images of a message that went.
    Forget(Vec<String>),
    /// Answers once what was asked before is on disk.
    Synced(mpsc::Sender<()>),
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
    /// Folder changes not yet handed to the writer, and whether the
    /// messages' file changed.
    writes: Vec<Write>,
    dirty: bool,
}

/// The file an image of message `id` is kept in.
fn image_file(id: u64, at: usize) -> String {
    format!("{id}-{at}")
}

impl Held {
    fn from_messages(mut messages: Vec<Message>) -> Self {
        // The file is written after each change, so a send cut short by
        // quitting or a crash may have gone even if it still reads as waiting.
        for message in &mut messages {
            if matches!(message.state, Stage::Waiting | Stage::Sending) {
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

    /// Keeps a message for `key`; true when its session needs a flusher
    /// started. A message behind others waiting for the machine is answered
    /// at once: it waits too.
    pub(crate) fn push(
        &mut self,
        key: &SessionKey,
        draft: Draft,
        reply: Option<oneshot::Sender<Answer>>,
    ) -> (u64, bool) {
        let id = self.next;
        self.next += 1;
        let images: Vec<KeptImage> = draft.images.into_iter().map(Into::into).collect();

        for (at, image) in images.iter().enumerate() {
            self.writes.push(Write::Image {
                file: image_file(id, at),
                data: image.data.clone(),
            });
        }

        let stuck = self.messages.iter().any(|m| {
            m.key() == *key && m.state == Stage::Waiting && !self.replies.contains_key(&m.id)
        });
        self.messages.push(Message {
            id,
            machine_id: key.machine_id.clone(),
            agent: key.agent.clone(),
            session: key.session.clone(),
            how: draft.how,
            text: draft.text,
            images,
            state: Stage::Waiting,
            unsure: false,
            seen: draft.seen,
        });
        self.dirty = true;

        if let Some(reply) = reply {
            if stuck {
                let _ = reply.send(Answer::Waiting);
            } else {
                self.replies.insert(id, reply);
            }
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
        self.dirty = true;
        Some(message.clone())
    }

    /// The machine took message `id`: it goes.
    pub(crate) fn sent(&mut self, id: u64, keys: Vec<String>) -> Option<Changed> {
        let message = self.remove(id)?;

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
        self.dirty = true;
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
        if let Some(reply) = self.replies.remove(&id) {
            self.remove(id)?;
            let _ = reply.send(Answer::Refused(reason));
            return None;
        }

        let message = self.messages.iter_mut().find(|m| m.id == id)?;
        message.state = Stage::Failed { reason };
        self.dirty = true;
        let key = message.key();
        Some(self.changed(&key))
    }

    /// The window that sent message `id` stopped waiting for its first try:
    /// from now on the outbox shows it.
    pub(crate) fn hand_over(&mut self, id: u64) -> Option<Changed> {
        self.replies.remove(&id)?;
        let key = self.messages.iter().find(|m| m.id == id)?.key();
        Some(self.changed(&key))
    }

    /// Takes message `id` out, unless it is being sent.
    pub(crate) fn take(&mut self, id: u64) -> Result<(Message, Changed)> {
        let message =
            self.messages.iter().find(|m| m.id == id).ok_or_else(|| {
                DesktopError::BadRequest("That message is no longer waiting.".into())
            })?;

        if message.state == Stage::Sending {
            return Err(DesktopError::BadRequest(
                "That message is being sent.".into(),
            ));
        }

        let message = self
            .remove(id)
            .ok_or_else(|| DesktopError::BadRequest("That message is no longer waiting.".into()))?;
        self.replies.remove(&id);
        let changed = self.changed(&message.key());
        Ok((message, changed))
    }

    /// Takes out every message of `key` not being sent, oldest first, in
    /// one step: the flusher cannot start one of them meanwhile.
    pub(crate) fn drain(&mut self, key: &SessionKey) -> (Vec<Message>, Changed) {
        let ids: Vec<u64> = self
            .messages
            .iter()
            .filter(|m| m.key() == *key && m.state != Stage::Sending)
            .map(|m| m.id)
            .collect();
        let taken = ids
            .into_iter()
            .filter_map(|id| {
                if let Some(reply) = self.replies.remove(&id) {
                    let _ = reply.send(Answer::Waiting);
                }
                self.remove(id)
            })
            .collect();
        (taken, self.changed(key))
    }

    /// Removes message `id` and forgets its images.
    fn remove(&mut self, id: u64) -> Option<Message> {
        let at = self.messages.iter().position(|m| m.id == id)?;
        let message = self.messages.remove(at);
        self.dirty = true;

        if !message.images.is_empty() {
            let files = (0..message.images.len())
                .map(|at| image_file(id, at))
                .collect();
            self.writes.push(Write::Forget(files));
        }

        Some(message)
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

/// The kept messages and the thread that writes them.
pub struct Outbox {
    held: Mutex<Held>,
    writes: mpsc::Sender<Write>,
}

impl Outbox {
    /// The outbox kept in `dir`, or an empty one.
    pub fn load(dir: PathBuf) -> Self {
        let messages = read_messages(&dir);
        let (writes, queue) = mpsc::channel();
        let spawned = thread::Builder::new()
            .name("outbox".into())
            .spawn(move || write_all(&dir, &queue));

        if let Err(e) = spawned {
            log::warn!("starting the outbox writer: {e}");
        }

        Self {
            held: Mutex::new(Held::from_messages(messages)),
            writes,
        }
    }

    /// Changes the held messages with `change`, then has what changed
    /// written: the messages' file without image bytes, serialized after
    /// the lock is let go.
    pub(crate) fn with<T>(&self, change: impl FnOnce(&mut Held) -> T) -> T {
        let (out, writes, messages) = {
            let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
            let out = change(&mut held);
            let writes = std::mem::take(&mut held.writes);
            let messages = std::mem::take(&mut held.dirty).then(|| {
                held.messages
                    .iter()
                    .map(|m| Message {
                        images: m
                            .images
                            .iter()
                            .map(|i| KeptImage {
                                data: String::new(),
                                ..i.clone()
                            })
                            .collect(),
                        ..m.clone()
                    })
                    .collect::<Vec<_>>()
            });
            (out, writes, messages)
        };

        for write in writes {
            let _ = self.writes.send(write);
        }

        if let Some(messages) = messages {
            match serde_json::to_vec(&messages) {
                Ok(bytes) => {
                    let _ = self.writes.send(Write::Messages(bytes));
                }
                Err(e) => log::warn!("keeping the outbox: {e}"),
            }
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

const MESSAGES: &str = "messages.json";
const IMAGES: &str = "images";

/// The messages kept in `dir`, each with its images read back. One whose
/// image is gone stays, refused, for the user to see.
fn read_messages(dir: &Path) -> Vec<Message> {
    let file = dir.join(MESSAGES);
    let mut messages: Vec<Message> = match fs::read(&file) {
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

    for message in &mut messages {
        let id = message.id;
        let mut lost = false;

        for (at, image) in message.images.iter_mut().enumerate() {
            match fs::read_to_string(dir.join(IMAGES).join(image_file(id, at))) {
                Ok(data) => image.data = data,
                Err(_) => lost = true,
            }
        }

        if lost {
            message.state = Stage::Failed {
                reason: "An image of this message could not be read.".into(),
            };
        }
    }

    messages
}

/// Does each change asked for, in order. Of several messages' files asked
/// for together only the newest is written, after the images it names.
fn write_all(dir: &Path, queue: &mpsc::Receiver<Write>) {
    let images = dir.join(IMAGES);
    let mut synced = Vec::new();

    while let Ok(first) = queue.recv() {
        let mut messages = None;

        for write in std::iter::once(first).chain(queue.try_iter()) {
            match write {
                Write::Messages(bytes) => messages = Some(bytes),
                Write::Image { file, data } => {
                    if let Err(e) = replace(&images, &file, data.as_bytes()) {
                        log::warn!("keeping an outbox image: {e}");
                    }
                }
                Write::Forget(files) => {
                    for file in files {
                        match fs::remove_file(images.join(&file)) {
                            Ok(()) => {}
                            Err(e) if e.kind() == ErrorKind::NotFound => {}
                            Err(e) => log::warn!("removing an outbox image: {e}"),
                        }
                    }
                }
                Write::Synced(done) => synced.push(done),
            }
        }

        if let Some(bytes) = messages
            && let Err(e) = replace(dir, MESSAGES, &bytes)
        {
            log::warn!("writing the outbox: {e}");
        }

        for done in synced.drain(..) {
            let _ = done.send(());
        }
    }
}

/// Replaces `dir/name` with `bytes` whole: written beside it, flushed to
/// the disk, then renamed over it.
fn replace(dir: &Path, name: &str, bytes: &[u8]) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let partial = dir.join(format!("{name}.part"));
    let mut file = File::create(&partial)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(&partial, dir.join(name))?;

    // The rename itself reaches the disk with the folder; Windows cannot
    // open a folder this way, and there the rename is kept as it is.
    #[cfg(unix)]
    File::open(dir)?.sync_all()?;
    Ok(())
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
        let log = Log {
            queued: &view.queued_messages,
            earlier: u64::from(view.earlier),
            entries: &view.entries,
        };
        Ok(holds(&log, message))
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

/// The part of a session read back after a dropped send.
struct Log<'a> {
    queued: &'a [String],
    /// Entries before `entries`.
    earlier: u64,
    entries: &'a [AgentEntry],
}

/// The text a session holds for a message waiting for its turn: its words,
/// then a line per image.
fn queued_form(message: &Message) -> String {
    std::iter::once(message.text.clone())
        .chain(
            message
                .images
                .iter()
                .map(|i| format!("[Image: {}]", i.name)),
        )
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether a user entry is `message`: the same words, whole, and a line per image.
fn is_message(entry: &str, message: &Message) -> bool {
    let (images, words): (Vec<&str>, Vec<&str>) = entry.lines().partition(|line| {
        line.trim_start()
            .to_ascii_lowercase()
            .starts_with("[image:")
    });
    words.join("\n").trim() == message.text.trim() && images.len() == message.images.len()
}

/// Whether a session, read after a send that may have gone, shows the
/// machine took `message`: the session holds more copies of it for the turn
/// than the window saw, or a user entry after what the window saw is it.
/// Without what the window saw, only the newest user entry counts.
fn holds(log: &Log, message: &Message) -> bool {
    let form = queued_form(message);
    let copies = |queued: &[String]| queued.iter().filter(|m| **m == form).count();
    let before = message.seen.as_ref().map_or(0, |seen| copies(&seen.queued));

    if copies(log.queued) > before {
        return true;
    }

    let mut users = log
        .entries
        .iter()
        .enumerate()
        .filter(|(_, e)| matches!(e.kind, AgentEntryKind::User {}));

    match &message.seen {
        Some(seen) => users
            .filter(|(at, _)| {
                u64::try_from(*at)
                    .ok()
                    .and_then(|at| log.earlier.checked_add(at))
                    .is_some_and(|at| at >= seen.entries)
            })
            .any(|(_, e)| is_message(&e.text, message)),
        None => users
            .next_back()
            .is_some_and(|(_, e)| is_message(&e.text, message)),
    }
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
/// waiting there: the machine's answer to its first try, or `Waiting` when
/// it waits for the machine or the try takes longer than `ANSWER_WAIT`.
pub(crate) async fn send(
    app: &AppHandle,
    window: &str,
    key: SessionKey,
    steer: Steer,
    seen: Option<Seen>,
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
    let draft = Draft {
        how,
        text,
        images,
        seen,
    };
    let (reply, mut answer) = oneshot::channel();
    let outbox = app.state::<Arc<Outbox>>();
    let (id, flusher) = outbox.with(|held| held.push(&key, draft, Some(reply)));

    if flusher {
        start(app, key, window.to_string());
    }

    let answer = match timeout(ANSWER_WAIT, &mut answer).await {
        Ok(answer) => answer.ok(),
        Err(_) => {
            // The outbox shows it from now on; an answer that came meanwhile still counts.
            if let Some(changed) = outbox.with(|held| held.hand_over(id)) {
                emit(app, changed);
            }

            answer.try_recv().ok()
        }
    };

    match answer {
        Some(Answer::Sent(keys)) => Ok(keys),
        Some(Answer::Refused(message)) => Err(SteerError::Failed { message }),
        Some(Answer::Waiting) | None => Err(SteerError::Waiting {
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
    Ok(message.taken())
}

/// Takes every waiting message of a session out at once, oldest first, as
/// Stop all puts them back in the field. One being sent is left to go.
#[tauri::command]
pub fn outbox_drain(
    app: AppHandle,
    outbox: State<'_, Arc<Outbox>>,
    machine_id: String,
    agent: String,
    session: String,
) -> Vec<Taken> {
    let key = SessionKey {
        machine_id,
        agent,
        session,
    };
    let (messages, changed) = outbox.with(|held| held.drain(&key));
    emit(&app, changed);
    messages.into_iter().map(Message::taken).collect()
}

#[cfg(test)]
#[path = "../tests/agents_outbox.rs"]
mod tests;
