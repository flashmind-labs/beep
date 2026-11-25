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
        kept_folder::{KeptFolder, KeptImage, Write, forget_images, image_writes},
        kept_transcripts::SessionKey,
        sessions::{SessionRow, closed, resume_session},
        steer::{BEFORE_STOP_ALL, PastedImage, Steer, SteerError, newer_than, sent},
    },
    error::{DesktopError, Result},
    fleet::{
        Fleet, agents as direct, alerts,
        asked::{ORDER_LIMIT, READ_LIMIT},
        events::Events,
        session_mirror::SessionMirror,
    },
};
use base64::{Engine, engine::general_purpose::STANDARD};
use beep_mesh::RETRY;
use beep_proto::SessionId;
use beep_proto::{
    AgentEntry, AgentEntryKind, AgentNoticeKey, AgentSteer, ApiError, GetAgentSession, ModelId,
    ResumeAgentSession, SteerAgentSession, WriteWorkFile,
    node::agent::{Effort, Request as AgentRequest, SessionDevice},
    node::events::{CallReply, Operation},
};
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};
use tokio::{
    sync::oneshot,
    time::{sleep, timeout},
};

/// The event every window hears when a session's outbox changes.
const CHANGED: &str = "outbox-changed";
/// How many entries a check after a dropped send reads.
const CHECK_LAST: u32 = 60;
/// How long a session order waits for a healthy local/fast node before handing
/// off to the session's background actor so a degraded link never stalls the UI.
const ANSWER_WAIT: Duration = Duration::from_millis(250);
/// How long transient interrupt orders (`Cancel`, `StopAll`, `StopTask`) stay queued if
/// a machine remains unreachable.
const ORDER_TTL: Duration = Duration::from_secs(60);
/// The bytes of a file one upload call carries, about: its base64 text is cut at
/// a multiple of four characters.
const UPLOAD_PART: usize = 2 * 1024 * 1024;
const PART_CHARS: usize = UPLOAD_PART / 3 * 4;
/// The largest file a message uploads. The window refuses larger ones too.
const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;

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
    /// Its words are kept, and its files are still being read by the window that sent
    /// it: nothing behind it goes before they arrive.
    Reading,
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

/// Files a message uploads to its session's machine before it goes, as the
/// window hands them over, and the paths it mentions without uploading. The
/// message's text then holds only what was typed: the mentions are added once
/// the files are there and have their paths.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Attach {
    /// The session's folder.
    pub(crate) root: String,
    pub(crate) files: Vec<PastedImage>,
    #[serde(default)]
    pub(crate) mentions: Vec<String>,
    /// Set when the window is still reading the files: the message is kept with its words
    /// now, and `outbox_attach` with this token completes it.
    #[serde(default)]
    pub(crate) reading: Option<String>,
}

/// Why a message fails whose files were still being read when Beep quit.
const READING_LOST: &str = "Beep quit while its files were read. Edit it to get its words back.";

/// Why a message fails that was resuming its session when Beep quit.
const RESUME_DOUBT: &str =
    "The session may have resumed before Beep quit. Check the session list, then send again.";

/// A session's model and thinking picks, applied once it runs again.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Pick {
    #[serde(default)]
    pub(crate) model: Option<ModelId>,
    #[serde(default)]
    pub(crate) effort: Option<Effort>,
    #[serde(default)]
    pub(crate) fast: Option<bool>,
}

/// A message to a session that ended: it resumes first, once however many
/// messages wait for it, and the messages go on in the session that resumes.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Resume {
    /// The model a Codex session resumes on.
    #[serde(default)]
    pub(crate) model: Option<ModelId>,
    /// What to apply to the resumed session before the message.
    #[serde(default)]
    pub(crate) pick: Option<Pick>,
    /// The session is running again; what is left is its pick.
    #[serde(default)]
    pub(crate) resumed: bool,
}

/// What a window read for a message it handed over with its files still unread.
#[derive(Debug, Deserialize)]
pub struct Read {
    text: String,
    #[serde(default)]
    images: Vec<PastedImage>,
    #[serde(default)]
    files: Vec<PastedImage>,
    #[serde(default)]
    mentions: Vec<String>,
    #[serde(default)]
    root: String,
}

/// A message as a window hands it to the outbox.
pub(crate) struct Draft {
    pub(crate) how: Kind,
    pub(crate) text: String,
    pub(crate) images: Vec<PastedImage>,
    /// Files to upload first, already kept: a whole file is copied before the lock, not under it.
    pub(crate) files: Vec<KeptImage>,
    pub(crate) root: String,
    pub(crate) mentions: Vec<String>,
    pub(crate) resume: Option<Resume>,
    pub(crate) seen: Option<Seen>,
    pub(crate) reading: Option<String>,
}

/// One message kept for a session.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Message {
    pub(crate) id: u64,
    pub(crate) machine_id: String,
    pub(crate) agent: String,
    pub(crate) session: SessionId,
    pub(crate) how: Kind,
    pub(crate) text: String,
    pub(crate) images: Vec<KeptImage>,
    /// Files still to upload, kept like the images; none once they are there.
    #[serde(default)]
    pub(crate) files: Vec<KeptImage>,
    /// The session's folder, where the files go.
    #[serde(default)]
    pub(crate) root: String,
    /// Paths to mention after the uploaded files' own.
    #[serde(default)]
    pub(crate) mentions: Vec<String>,
    /// Its session ended: it resumes before the message goes.
    #[serde(default)]
    pub(crate) resume: Option<Resume>,
    /// The session this one went on from, when its session was resumed for a message: kept
    /// so that after a restart an order or a read for the ended session still finds this one.
    #[serde(default)]
    pub(crate) resumed_from: Option<SessionId>,
    pub(crate) state: Stage,
    /// A send of it may have reached the machine before the link dropped,
    /// so the session is read before it is sent again.
    pub(crate) unsure: bool,
    #[serde(default)]
    pub(crate) seen: Option<Seen>,
    /// The token its window completes it with while its files are read.
    #[serde(default)]
    pub(crate) reading: Option<String>,
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
            Stage::Reading => ("reading", None),
            Stage::Sending if self.resume.as_ref().is_some_and(|r| !r.resumed) => {
                ("resuming", None)
            }
            Stage::Sending if !self.files.is_empty() => ("uploading", None),
            Stage::Sending => ("sending", None),
            Stage::Failed { reason } => ("failed", Some(reason.clone())),
        };
        Row {
            id: self.id,
            how: self.how,
            text: self.text.clone(),
            images: self
                .images
                .iter()
                .chain(&self.files)
                .map(|image| image.name.clone())
                .collect(),
            state,
            reason,
            keys: Vec::new(),
        }
    }

    /// Nothing to send but the resume of its session: a menu action, since no message of the
    /// field is ever empty.
    fn bare(&self) -> bool {
        self.text.is_empty()
            && self.images.is_empty()
            && self.files.is_empty()
            && self.mentions.is_empty()
    }

    /// Nothing of it has gone: its session is still to resume and its files to upload.
    fn untouched(&self) -> bool {
        self.files.is_empty() && self.resume.is_none()
    }

    fn taken(self) -> Taken {
        Taken {
            text: self.text,
            images: self.images.into_iter().map(Into::into).collect(),
            files: self.files.into_iter().map(Into::into).collect(),
            mentions: self.mentions,
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
    /// "waiting", "reading", "resuming", "uploading", "sending", "sent" or "failed". A sent row stays
    /// until the session's own push lists the message.
    pub(crate) state: &'static str,
    pub(crate) reason: Option<String>,
    /// For a row in `Changed.sent`: the keys the machine gave its images.
    pub(crate) keys: Vec<String>,
}

/// The most `sent` rows kept for sessions no window follows.
const MAX_DELIVERED: usize = 64;

/// What a session's outbox holds now, and the kept messages that went to
/// the machine since it last changed.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Changed {
    /// Taken under the outbox lock: the window draws the rows of the highest it has seen.
    pub(crate) revision: u64,
    pub(crate) machine_id: String,
    pub(crate) agent: String,
    pub(crate) session: SessionId,
    pub(crate) rows: Vec<Row>,
    pub(crate) sent: Vec<Row>,
    /// The session that goes on from this one, which ended and was resumed:
    /// the windows showing this one open it.
    pub(crate) resumed: Option<SessionRow>,
    /// The pick the message that resumed it asked for, which the session runs on.
    pub(crate) pick: Option<Pick>,
    /// The files that just reached the machine, and where.
    pub(crate) uploaded: Vec<Uploaded>,
}

/// What `outbox_list` answers: the rows, and the revision they are as of.
#[derive(Debug, Serialize)]
pub(crate) struct Listed {
    pub(crate) revision: u64,
    /// The session the rows are of: the one asked for, or the one it was resumed as.
    pub(crate) session: SessionId,
    pub(crate) rows: Vec<Row>,
}

/// A file of a message, on the machine at `path`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Uploaded {
    pub(crate) name: String,
    pub(crate) path: String,
}

/// A non-message session control action or question answer handled by the session actor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum OrderKind {
    Configure {
        model: Option<ModelId>,
        effort: Option<Effort>,
        fast: Option<bool>,
    },
    Mode {
        mode: String,
    },
    Device {
        device: Option<SessionDevice>,
    },
    Cancel,
    StopAll,
    Close,
    StopTask {
        task: String,
    },
    Answer {
        question: u32,
        reply: String,
    },
}

impl OrderKind {
    /// Whether it goes out while a message of its session resumes or uploads.
    fn urgent(&self) -> bool {
        matches!(
            self,
            Self::Cancel
                | Self::StopAll
                | Self::StopTask { .. }
                | Self::Close
                | Self::Answer { .. }
        )
    }

    fn ttl(&self) -> Option<Duration> {
        match self {
            Self::Cancel | Self::StopAll | Self::StopTask { .. } => Some(ORDER_TTL),
            Self::Configure { .. }
            | Self::Mode { .. }
            | Self::Device { .. }
            | Self::Close
            | Self::Answer { .. } => None,
        }
    }
}

/// One transient control order queued for a session in memory.
#[derive(Clone, Debug)]
pub(crate) struct Order {
    pub(crate) id: u64,
    pub(crate) key: SessionKey,
    pub(crate) kind: OrderKind,
    pub(crate) sending: bool,
    pub(crate) deadline: Option<Instant>,
}

/// Outcome of submitting an [`Order`] to the session actor.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum OrderAnswer {
    Sent,
    Queued,
    Refused(String),
}

/// Next unit of work for a session's actor, in causal `id` order.
#[derive(Clone, Debug)]
pub(crate) enum Step {
    Order(Order),
    Message(Box<Message>),
}

/// The kept messages, transient session orders, and what is being done with them.
#[derive(Default)]
pub(crate) struct Held {
    messages: Vec<Message>,
    orders: Vec<Order>,
    next: u64,
    /// Sessions whose flusher runs.
    flushing: HashSet<SessionKey>,
    /// Sessions whose urgent lane runs: it sends their urgent orders while their
    /// flusher is held up by a resume or an upload.
    urgent: HashSet<SessionKey>,
    /// Where a session that was resumed went on: a window that has not
    /// opened that yet still sends to the old one, and is sent on.
    redirects: HashMap<SessionKey, SessionKey>,
    /// Callers waiting briefly on an order's first try.
    order_replies: HashMap<u64, oneshot::Sender<OrderAnswer>>,
    /// Folder changes not yet handed to the writer, and whether the
    /// messages' file changed.
    writes: Vec<Write>,
    dirty: bool,
    /// The last revision a `Changed` took.
    revision: u64,
    /// Messages the machine took, shown as `sent` rows until a push of their session
    /// lists them. Kept in memory only: a restart has nothing left to show.
    delivered: Vec<Message>,
}

impl Held {
    fn from_messages(mut messages: Vec<Message>) -> Self {
        // The file is written after each change, so a send cut short by
        // quitting or a crash may have gone even if it still reads as waiting.
        // A resume cut short by quitting or a crash may have reached the machine, and
        // sending it again would branch a second session: the message and those
        // waiting behind it for that session fail, for the user to look.
        let mut doubtful: Vec<SessionKey> = Vec::new();

        for message in &mut messages {
            if message.state == Stage::Reading {
                message.state = Stage::Failed {
                    reason: READING_LOST.into(),
                };
            }
        }

        for message in &mut messages {
            if message.state == Stage::Sending
                && message.resume.as_ref().is_some_and(|r| !r.resumed)
            {
                message.state = Stage::Failed {
                    reason: RESUME_DOUBT.into(),
                };
                doubtful.push(message.key());
            }
        }

        for message in &mut messages {
            if message.state == Stage::Waiting && doubtful.contains(&message.key()) {
                message.state = Stage::Failed {
                    reason: RESUME_DOUBT.into(),
                };
            }
        }

        for message in &mut messages {
            if matches!(message.state, Stage::Waiting | Stage::Sending) {
                message.state = Stage::Waiting;
                // Nothing of it went while its session or files were still to do.
                message.unsure = message.untouched();
            }
        }

        let next = messages.iter().map(|m| m.id).max().map_or(0, |id| id + 1);
        let redirects = messages
            .iter()
            .filter_map(|m| {
                let from = SessionKey {
                    session: m.resumed_from.clone()?,
                    ..m.key()
                };
                Some((from, m.key()))
            })
            .collect();
        Self {
            messages,
            next,
            redirects,
            ..Self::default()
        }
    }

    /// Keeps a message for `key`; true when its session needs a flusher
    /// started. A message behind others waiting for the machine is answered
    /// at once: it waits too.
    pub(crate) fn push(&mut self, key: &SessionKey, draft: Draft) -> (u64, bool) {
        let resolved = self.resolve(key);
        // One resume however many messages wait: a message behind another
        // that resumes, or to a session already resumed, never does.
        let resume = draft
            .resume
            .filter(|_| resolved == *key && !self.resuming(key));
        let key = &resolved;
        let id = self.next;
        self.next += 1;
        let images: Vec<KeptImage> = draft.images.into_iter().map(Into::into).collect();

        self.writes.extend(image_writes(id, &images));
        let files = draft.files;
        self.writes.extend(image_writes(files_id(id), &files));

        self.messages.push(Message {
            id,
            machine_id: key.machine_id.clone(),
            agent: key.agent.clone(),
            session: key.session.clone(),
            how: draft.how,
            text: draft.text,
            images,
            files,
            root: draft.root,
            mentions: draft.mentions,
            resume,
            resumed_from: None,
            state: if draft.reading.is_some() {
                Stage::Reading
            } else {
                Stage::Waiting
            },
            unsure: false,
            seen: draft.seen,
            reading: draft.reading,
        });
        self.dirty = true;

        (id, self.flushing.insert(key.clone()))
    }

    /// The session `key` went on in, if it was resumed.
    pub(crate) fn resolve(&self, key: &SessionKey) -> SessionKey {
        let mut key = key;

        // A chain ends where a session was not resumed; bounded in case of a loop.
        for _ in 0..self.redirects.len() {
            match self.redirects.get(key) {
                Some(next) => key = next,
                None => break,
            }
        }

        key.clone()
    }

    /// The resume of a message that left the queue without resuming goes to the
    /// next one waiting for the same session, unless another still carries one.
    fn hand_on_resume(&mut self, key: &SessionKey, resume: Option<Resume>) {
        let Some(resume) = resume.filter(|r| !r.resumed) else {
            return;
        };

        if self.resuming(key) {
            return;
        }

        if let Some(next) = self
            .messages
            .iter_mut()
            .find(|m| m.key() == *key && m.state == Stage::Waiting && m.resume.is_none())
        {
            next.resume = Some(resume);
            self.dirty = true;
        }
    }

    /// The outcome of message `id`'s resume is not known: it and every message
    /// waiting for its session fail with `reason`, so none goes to a session that
    /// ended or resumes it a second time.
    pub(crate) fn uncertain(&mut self, id: u64, reason: String) -> Option<Changed> {
        let key = self.messages.iter().find(|m| m.id == id)?.key();
        let ids: Vec<u64> = self
            .messages
            .iter()
            .filter(|m| m.key() == key && (m.id == id || m.state == Stage::Waiting))
            .map(|m| m.id)
            .collect();

        for id in ids {
            if let Some(message) = self.messages.iter_mut().find(|m| m.id == id) {
                message.state = Stage::Failed {
                    reason: reason.clone(),
                };
            }
        }

        self.dirty = true;
        Some(self.changed(&key))
    }

    /// Whether a message of `key` still to be sent resumes its session.
    fn resuming(&self, key: &SessionKey) -> bool {
        self.messages.iter().any(|m| {
            m.key() == *key
                && !matches!(m.state, Stage::Failed { .. })
                && m.resume.as_ref().is_some_and(|r| !r.resumed)
        })
    }

    /// Enqueues or coalesces a control `Order` for `key`, ordered with any queued messages.
    pub(crate) fn push_order(
        &mut self,
        key: &SessionKey,
        kind: OrderKind,
        now: Instant,
        reply: Option<oneshot::Sender<OrderAnswer>>,
    ) -> (u64, Option<Changed>, bool) {
        let outbox_changed = None;
        let after_message = self
            .messages
            .iter()
            .filter(|m| m.key() == *key && m.state == Stage::Waiting)
            .map(|m| m.id)
            .max();

        match &kind {
            OrderKind::Configure {
                model,
                effort,
                fast,
            } => {
                if let Some(existing) = self.orders.iter_mut().find(|o| {
                    o.key == *key
                        && !o.sending
                        && after_message.is_none_or(|mid| o.id > mid)
                        && matches!(o.kind, OrderKind::Configure { .. })
                }) {
                    if let OrderKind::Configure {
                        model: prev_model,
                        effort: prev_effort,
                        fast: prev_fast,
                    } = &mut existing.kind
                    {
                        if model.is_some() {
                            *prev_model = model.clone();
                        }
                        if effort.is_some() {
                            *prev_effort = *effort;
                        }
                        if fast.is_some() {
                            *prev_fast = *fast;
                        }
                    }
                    let id = existing.id;
                    self.replace_order_reply(id, reply, false);
                    return (id, None, self.flushing.insert(key.clone()));
                }
            }
            OrderKind::Mode { mode } => {
                if let Some(existing) = self.orders.iter_mut().find(|o| {
                    o.key == *key
                        && !o.sending
                        && after_message.is_none_or(|mid| o.id > mid)
                        && matches!(o.kind, OrderKind::Mode { .. })
                }) {
                    existing.kind = OrderKind::Mode { mode: mode.clone() };
                    let id = existing.id;
                    self.replace_order_reply(id, reply, false);
                    return (id, None, self.flushing.insert(key.clone()));
                }
            }
            OrderKind::Device { device } => {
                if let Some(existing) = self.orders.iter_mut().find(|o| {
                    o.key == *key
                        && !o.sending
                        && after_message.is_none_or(|mid| o.id > mid)
                        && matches!(o.kind, OrderKind::Device { .. })
                }) {
                    existing.kind = OrderKind::Device {
                        device: device.clone(),
                    };
                    let id = existing.id;
                    self.replace_order_reply(id, reply, false);
                    return (id, None, self.flushing.insert(key.clone()));
                }
            }
            OrderKind::Answer {
                question,
                reply: text,
            } => {
                if let Some(existing) = self.orders.iter_mut().find(|o| {
                    o.key == *key
                        && !o.sending
                        && matches!(o.kind, OrderKind::Answer { question: q, .. } if q == *question)
                }) {
                    existing.kind = OrderKind::Answer {
                        question: *question,
                        reply: text.clone(),
                    };
                    let id = existing.id;
                    self.replace_order_reply(id, reply, false);
                    return (id, None, self.flushing.insert(key.clone()));
                }
            }
            OrderKind::StopAll => {
                let drop_ids: Vec<u64> = self
                    .orders
                    .iter()
                    .filter(|o| {
                        o.key == *key
                            && !o.sending
                            && matches!(o.kind, OrderKind::Cancel | OrderKind::StopAll)
                    })
                    .map(|o| o.id)
                    .collect();

                for drop_id in drop_ids {
                    self.remove_order(drop_id, OrderAnswer::Queued);
                }
            }
            OrderKind::Close => {
                let drop_ids: Vec<u64> = self
                    .orders
                    .iter()
                    .filter(|o| o.key == *key && !o.sending)
                    .map(|o| o.id)
                    .collect();

                for drop_id in drop_ids {
                    self.remove_order(drop_id, OrderAnswer::Queued);
                }
            }
            OrderKind::Cancel | OrderKind::StopTask { .. } => {}
        }

        let stuck = self.is_stuck(key);
        let id = self.next;
        self.next += 1;
        let deadline = kind.ttl().map(|ttl| now + ttl);
        self.orders.push(Order {
            id,
            key: key.clone(),
            kind,
            sending: false,
            deadline,
        });
        self.replace_order_reply(id, reply, stuck);

        (id, outbox_changed, self.flushing.insert(key.clone()))
    }

    /// Whether a message or an order of `key` waits for its machine, so a new order waits too.
    fn is_stuck(&self, key: &SessionKey) -> bool {
        self.messages
            .iter()
            .any(|m| m.key() == *key && m.state == Stage::Waiting)
            || self
                .orders
                .iter()
                .any(|o| o.key == *key && !o.sending && !self.order_replies.contains_key(&o.id))
    }

    fn replace_order_reply(
        &mut self,
        id: u64,
        reply: Option<oneshot::Sender<OrderAnswer>>,
        stuck: bool,
    ) {
        if let Some(prev) = self.order_replies.remove(&id) {
            let _ = prev.send(OrderAnswer::Queued);
        }

        if let Some(reply) = reply {
            if stuck {
                let _ = reply.send(OrderAnswer::Queued);
            } else {
                self.order_replies.insert(id, reply);
            }
        }
    }

    fn remove_order(&mut self, id: u64, answer: OrderAnswer) {
        if let Some(at) = self.orders.iter().position(|o| o.id == id) {
            self.orders.remove(at);
        }

        if let Some(waiter) = self.order_replies.remove(&id) {
            let _ = waiter.send(answer);
        }
    }

    /// Next order or message for `key` in `id` order; `None` ends its flusher.
    pub(crate) fn begin_step(&mut self, key: &SessionKey, now: Instant) -> Option<Step> {
        self.expire(key, now);

        if let Some(close_id) = self
            .orders
            .iter()
            .find(|o| o.key == *key && !o.sending && matches!(o.kind, OrderKind::Close))
            .map(|o| o.id)
        {
            return self.begin_order(close_id).map(Step::Order);
        }

        let order_id = self
            .orders
            .iter()
            .find(|o| o.key == *key && !o.sending)
            .map(|o| o.id);
        let msg_id = self.next_to_send(key);

        match (order_id, msg_id) {
            (Some(oid), Some(mid)) if oid < mid => self.begin_order(oid).map(Step::Order),
            (Some(oid), None) => self.begin_order(oid).map(Step::Order),
            (_, Some(_)) => self
                .begin(key)
                .map(|message| Step::Message(Box::new(message))),
            (None, None) => {
                self.flushing.remove(key);
                None
            }
        }
    }

    /// The oldest message of `key` to send. One whose files are still read holds back those
    /// behind it, so they go in the order typed.
    fn next_to_send(&self, key: &SessionKey) -> Option<u64> {
        self.messages
            .iter()
            .filter(|m| m.key() == *key)
            .find(|m| matches!(m.state, Stage::Waiting | Stage::Reading))
            .filter(|m| m.state == Stage::Waiting)
            .map(|m| m.id)
    }

    /// Drops the orders of `key` that waited out their time: a Stop pressed long ago
    /// must not stop a turn started since.
    fn expire(&mut self, key: &SessionKey, now: Instant) {
        let expired: Vec<u64> = self
            .orders
            .iter()
            .filter(|o| o.key == *key && !o.sending && o.deadline.is_some_and(|d| now >= d))
            .map(|o| o.id)
            .collect();

        for id in expired {
            self.remove_order(id, OrderAnswer::Queued);
        }
    }

    /// Whether a message of `key` is uploading its files, or applying its picks, in the
    /// session it has: one still resuming has no final session yet, so the orders wait
    /// for `resumed` to move them.
    fn slow(&self, key: &SessionKey) -> bool {
        self.messages.iter().any(|m| {
            m.key() == *key
                && m.state == Stage::Sending
                && m.resume.as_ref().is_none_or(|r| r.resumed)
                && (m.resume.is_some() || !m.files.is_empty())
        })
    }

    /// Whether `key` needs an urgent lane started: an urgent order waits and
    /// its flusher is held up.
    pub(crate) fn start_urgent(&mut self, key: &SessionKey) -> bool {
        self.slow(key)
            && self
                .orders
                .iter()
                .any(|o| o.key == *key && !o.sending && o.kind.urgent())
            && self.urgent.insert(key.clone())
    }

    /// The oldest urgent order of `key`, while its flusher is held up; `None`
    /// ends the urgent lane, and the flusher takes what is left.
    pub(crate) fn begin_urgent(&mut self, key: &SessionKey, now: Instant) -> Option<Order> {
        let key = self.resolve(key);
        self.expire(&key, now);
        let id = self
            .orders
            .iter()
            .find(|o| o.key == key && !o.sending && o.kind.urgent())
            .map(|o| o.id)
            .filter(|_| self.slow(&key));

        match id {
            Some(id) => self.begin_order(id),
            None => {
                self.urgent.remove(&key);
                None
            }
        }
    }

    fn begin_order(&mut self, id: u64) -> Option<Order> {
        let order = self.orders.iter_mut().find(|o| o.id == id && !o.sending)?;
        order.sending = true;
        Some(order.clone())
    }

    pub(crate) fn order_sent(&mut self, id: u64) -> Option<Changed> {
        let sent = self
            .orders
            .iter()
            .find(|o| o.id == id)
            .map(|o| (o.key.clone(), o.kind.clone()));
        self.remove_order(id, OrderAnswer::Sent);
        let (key, kind) = sent?;
        let resuming = self.messages.iter().any(|m| {
            m.key() == key && m.resume.is_some() && !matches!(m.state, Stage::Failed { .. })
        });
        let dropped = match kind {
            // A closed session takes its undelivered messages with it, whatever they do now.
            OrderKind::Close => {
                let sent = self.forget_delivered(&key);
                self.drop_undelivered(&key, true) || sent
            }
            // A Stop while the session resumes is for what was just sent: it goes unsent.
            // Stop all also drops the steers the machine holds.
            OrderKind::Cancel | OrderKind::StopAll => {
                let sent = matches!(kind, OrderKind::StopAll) && self.forget_delivered(&key);
                (resuming && self.drop_undelivered(&key, false)) || sent
            }
            _ => false,
        };

        dropped.then(|| self.changed(&key))
    }

    /// Removes the messages of `key` that were not delivered, those being sent included:
    /// the flusher finds its message gone after its next await and stops. `failed` takes
    /// the ones kept with a reason too.
    fn drop_undelivered(&mut self, key: &SessionKey, failed: bool) -> bool {
        let ids: Vec<u64> = self
            .messages
            .iter()
            .filter(|m| m.key() == *key && (failed || !matches!(m.state, Stage::Failed { .. })))
            .map(|m| m.id)
            .collect();

        for id in &ids {
            self.remove(*id);
        }

        !ids.is_empty()
    }

    /// Whether message `id` is still kept.
    pub(crate) fn holds(&self, id: u64) -> bool {
        self.messages.iter().any(|m| m.id == id)
    }

    pub(crate) fn order_wait(&mut self, id: u64, now: Instant) {
        let Some(key) = self
            .orders
            .iter()
            .find(|o| o.id == id)
            .map(|o| o.key.clone())
        else {
            return;
        };

        if let Some(order) = self.orders.iter_mut().find(|o| o.id == id) {
            if order.deadline.is_some_and(|d| now >= d) {
                self.remove_order(id, OrderAnswer::Queued);
            } else {
                order.sending = false;
            }
        }

        let order_ids: Vec<u64> = self
            .orders
            .iter()
            .filter(|o| o.key == key)
            .map(|o| o.id)
            .collect();

        for oid in order_ids {
            if let Some(waiter) = self.order_replies.remove(&oid) {
                let _ = waiter.send(OrderAnswer::Queued);
            }
        }
    }

    /// Removes a refused order; returns `true` if no foreground caller was still waiting.
    pub(crate) fn order_refused(&mut self, id: u64, reason: String) -> bool {
        if let Some(at) = self.orders.iter().position(|o| o.id == id) {
            self.orders.remove(at);
        }

        if let Some(waiter) = self.order_replies.remove(&id) {
            let _ = waiter.send(OrderAnswer::Refused(reason));
            false
        } else {
            true
        }
    }

    pub(crate) fn detach_order(&mut self, id: u64) {
        self.order_replies.remove(&id);
    }

    /// The oldest message of `key` to send, marked as sending; `None` ends
    /// its flusher.
    pub(crate) fn begin(&mut self, key: &SessionKey) -> Option<Message> {
        let Some(message) = self
            .next_to_send(key)
            .and_then(|id| self.messages.iter_mut().find(|m| m.id == id))
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
        let key = message.key();
        let row = Row {
            keys,
            ..message.row()
        };
        // A resume with no words has nothing for the session to list.
        if !message.bare() {
            self.delivered.push(message);
        }

        if self.delivered.len() > MAX_DELIVERED {
            self.delivered.remove(0);
        }

        let mut changed = self.changed(&key);
        changed.sent.push(row);
        Some(changed)
    }

    /// The files of the message waiting under `token` are read: it gets them and its final
    /// words, and goes in its turn. `None` when it is gone, taken back meanwhile. Also whether
    /// its session needs a flusher started.
    pub(crate) fn attach(
        &mut self,
        token: &str,
        read: Read,
    ) -> Option<(Changed, SessionKey, bool)> {
        let message = self
            .messages
            .iter_mut()
            .find(|m| m.state == Stage::Reading && m.reading.as_deref() == Some(token))?;
        let id = message.id;
        let images: Vec<KeptImage> = read.images.into_iter().map(Into::into).collect();
        let files: Vec<KeptImage> = read.files.into_iter().map(Into::into).collect();

        message.text = read.text;
        message.mentions = read.mentions;
        message.root = read.root;
        message.images = images.clone();
        message.files = files.clone();
        message.state = Stage::Waiting;
        message.reading = None;
        let key = message.key();

        self.writes.extend(image_writes(id, &images));
        self.writes.extend(image_writes(files_id(id), &files));
        self.dirty = true;
        let flusher = self.flushing.insert(key.clone());

        Some((self.changed(&key), key, flusher))
    }

    /// The window gave up reading the files of the message under `token`: it goes.
    pub(crate) fn cancel(&mut self, token: &str) -> Option<Changed> {
        let id = self
            .messages
            .iter()
            .find(|m| m.state == Stage::Reading && m.reading.as_deref() == Some(token))?
            .id;
        let message = self.remove(id)?;

        Some(self.changed(&message.key()))
    }

    /// A push of `key`'s session holds `log`: the sent messages it lists leave their rows, the
    /// session's own queue and log show them from here.
    pub(crate) fn settled(&mut self, key: &SessionKey, log: &Log) -> Option<Changed> {
        let before = self.delivered.len();
        self.delivered
            .retain(|message| message.key() != *key || !holds(log, message));

        (self.delivered.len() != before).then(|| self.changed(key))
    }

    /// Forgets the sent rows of `key`: the machine dropped what it held, or the session closed.
    pub(crate) fn dropped(&mut self, key: &SessionKey) -> Option<Changed> {
        self.forget_delivered(key).then(|| self.changed(key))
    }

    fn forget_delivered(&mut self, key: &SessionKey) -> bool {
        let before = self.delivered.len();
        self.delivered.retain(|message| message.key() != *key);

        self.delivered.len() != before
    }

    /// The files of message `id` are on the machine, at `paths`: they and the
    /// paths it mentions go into its text, and it is sent as it now is.
    pub(crate) fn uploaded(&mut self, id: u64, paths: Vec<String>) -> Option<(Message, Changed)> {
        let message = self.messages.iter_mut().find(|m| m.id == id)?;
        let files = std::mem::take(&mut message.files);
        let images = message.images.len();
        let names: Vec<String> = files.iter().map(|f| f.name.clone()).collect();
        let mut mentions = std::mem::take(&mut message.mentions);
        let only_pictures =
            mentions.is_empty() && files.iter().all(|f| f.mime.starts_with("image/"));
        let uploaded = names
            .into_iter()
            .zip(&paths)
            .map(|(name, path)| Uploaded {
                name,
                path: path.clone(),
            })
            .collect();
        mentions.extend(paths);
        message.text = with_mentions(&message.text, &mentions, images, only_pictures);
        let (message, key) = (message.clone(), message.key());

        self.writes.extend(forget_images(files_id(id), files.len()));
        self.dirty = true;
        let mut changed = self.changed(&key);
        changed.uploaded = uploaded;
        Some((message, changed))
    }

    /// Message `id`'s session was resumed, as `session`: it and every message
    /// and order kept for the old session go on there, and later sends to the
    /// old one are sent on. The old session's flusher goes on as the new one's.
    /// The changes of the old session and of the new one.
    pub(crate) fn resumed(
        &mut self,
        id: u64,
        session: SessionId,
    ) -> Option<(Message, Changed, Changed)> {
        let from = self.messages.iter().find(|m| m.id == id)?.key();
        let to = SessionKey {
            session,
            ..from.clone()
        };

        if from != to {
            for message in self.messages.iter_mut().filter(|m| m.key() == from) {
                message.resumed_from = Some(from.session.clone());
                message.session = to.session.clone();
                message.resume = if message.id == id {
                    message.resume.take().map(|r| Resume { resumed: true, ..r })
                } else {
                    None
                };
            }

            for order in self.orders.iter_mut().filter(|o| o.key == from) {
                order.key = to.clone();
            }

            self.flushing.remove(&from);
            self.flushing.insert(to.clone());

            if self.urgent.remove(&from) {
                self.urgent.insert(to.clone());
            }

            self.redirects.insert(from.clone(), to.clone());

            // An order of the old session could not go until now: its time starts here.
            let now = Instant::now();
            for order in self.orders.iter_mut().filter(|o| o.key == to) {
                order.deadline = order.kind.ttl().map(|ttl| now + ttl);
            }
        } else if let Some(resume) = self
            .messages
            .iter_mut()
            .find(|m| m.id == id)
            .and_then(|m| m.resume.as_mut())
        {
            resume.resumed = true;
        }

        self.dirty = true;
        let message = self.messages.iter().find(|m| m.id == id)?.clone();
        Some((message, self.changed(&from), self.changed(&to)))
    }

    /// The pick of message `id`'s resumed session is applied: nothing is left to do first.
    /// False when it is gone.
    pub(crate) fn resume_done(&mut self, id: u64) -> bool {
        let Some(message) = self.messages.iter_mut().find(|m| m.id == id) else {
            return false;
        };
        message.resume = None;
        self.dirty = true;
        true
    }

    /// Message `id` could not reach its machine: it and those after it wait,
    /// and the windows that sent them are told so.
    pub(crate) fn wait(&mut self, id: u64, unsure: bool) -> Option<Changed> {
        let message = self.messages.iter_mut().find(|m| m.id == id)?;
        message.state = Stage::Waiting;
        message.unsure = unsure;
        self.dirty = true;
        let key = message.key();
        Some(self.changed(&key))
    }

    /// The machine refused message `id`: it stays with the reason.
    pub(crate) fn refused(&mut self, id: u64, reason: String) -> Option<Changed> {
        let message = self.messages.iter_mut().find(|m| m.id == id)?;
        message.state = Stage::Failed { reason };
        self.dirty = true;
        let (key, resume) = (message.key(), message.resume.clone());
        self.hand_on_resume(&key, resume);
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
        self.hand_on_resume(&message.key(), message.resume.clone());
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
        let mut taken: Vec<Message> = ids.into_iter().filter_map(|id| self.remove(id)).collect();
        // What the machine took but no push listed yet: Stop all drops it there too.
        taken.extend(self.delivered.iter().filter(|m| m.key() == *key).cloned());
        self.forget_delivered(key);
        taken.sort_by_key(|m| m.id);
        (taken, self.changed(key))
    }

    /// Removes message `id` and forgets its images.
    fn remove(&mut self, id: u64) -> Option<Message> {
        let at = self.messages.iter().position(|m| m.id == id)?;
        let message = self.messages.remove(at);
        self.dirty = true;

        self.writes.extend(forget_images(id, message.images.len()));
        self.writes
            .extend(forget_images(files_id(id), message.files.len()));

        Some(message)
    }

    /// The rows the windows show for `key`, in the order typed.
    pub(crate) fn rows(&self, key: &SessionKey) -> Vec<Row> {
        self.messages
            .iter()
            .filter(|m| m.key() == *key)
            .map(Message::row)
            .chain(
                self.delivered
                    .iter()
                    .filter(|m| m.key() == *key)
                    .map(|m| Row {
                        state: "sent",
                        ..m.row()
                    }),
            )
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

    /// What `outbox_list` answers for `key`, or the session it went on in.
    pub(crate) fn listed(&self, key: &SessionKey) -> Listed {
        let key = self.resolve(key);
        Listed {
            revision: self.revision,
            rows: self.rows(&key),
            session: key.session,
        }
    }

    fn changed(&mut self, key: &SessionKey) -> Changed {
        self.revision += 1;
        Changed {
            revision: self.revision,
            machine_id: key.machine_id.clone(),
            agent: key.agent.clone(),
            session: key.session.clone(),
            rows: self.rows(key),
            sent: Vec::new(),
            resumed: None,
            pick: None,
            uploaded: Vec::new(),
        }
    }
}

/// The kept messages and the thread that writes them.
pub struct Outbox {
    held: Mutex<Held>,
    folder: KeptFolder,
}

impl Outbox {
    /// The outbox kept in `dir`, or an empty one.
    pub fn load(dir: PathBuf, cache: PathBuf) -> Self {
        let folder = KeptFolder::open(dir, cache, MESSAGES, "outbox");
        let messages = read_messages(&folder);
        Self {
            held: Mutex::new(Held::from_messages(messages)),
            folder,
        }
    }

    /// Changes the held messages with `change`, then has what changed
    /// written: the messages' file, which leaves out image bytes, serialized
    /// and queued under the lock so no waiting image is copied and the
    /// files land in the order of the changes.
    pub(crate) fn with<T>(&self, change: impl FnOnce(&mut Held) -> T) -> T {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        let out = change(&mut held);
        let file = std::mem::take(&mut held.dirty)
            .then(|| self.folder.records(&held.messages))
            .flatten();
        let writes: Vec<Write> = std::mem::take(&mut held.writes)
            .into_iter()
            .chain(file)
            .collect();

        // Queued before the lock goes, so a later change cannot be queued first.
        self.folder.write(writes);

        out
    }

    fn read<T>(&self, read: impl FnOnce(&Held) -> T) -> T {
        read(&self.held.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// Returns once what changed so far is on disk.
    pub fn sync(&self) {
        self.folder.sync();
    }
}

const MESSAGES: &str = "messages.json";

/// The bytes a base64 text holds.
fn base64_size(data: &str) -> u64 {
    let padding = data.bytes().rev().take_while(|b| *b == b'=').count().min(2);
    let len = u64::try_from(data.len()).unwrap_or(u64::MAX);

    (len / 4).saturating_mul(3).saturating_sub(padding as u64)
}

/// Why the first of `files` larger than `limit` bytes is refused, if any is.
fn too_big(files: &[KeptImage], limit: u64) -> Option<String> {
    files
        .iter()
        .find(|f| base64_size(&f.data) > limit)
        .map(|f| {
            format!(
                "{} is larger than {} MiB, the most a message uploads.",
                f.name,
                limit / (1024 * 1024)
            )
        })
}

/// What names the cached files of message `id`, apart from its images.
fn files_id(id: u64) -> String {
    format!("{id}f")
}

/// The text a message sends once its files have their paths: what was typed,
/// else a request for a message of pictures only, then the mentions.
fn with_mentions(text: &str, mentions: &[String], images: usize, only_pictures: bool) -> String {
    let count = images.max(mentions.len());
    let request = match text.trim() {
        "" if count > 0 && only_pictures && count == 1 => "Check this image.",
        "" if count > 0 && only_pictures => "Check these images.",
        typed => typed,
    };
    let mentioned = mentions
        .iter()
        .map(|path| {
            if path.contains(char::is_whitespace) {
                format!("@\"{path}\"")
            } else {
                format!("@{path}")
            }
        })
        .collect::<Vec<_>>()
        .join(" ");

    [request, mentioned.as_str()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// The messages kept in `folder`, each with its images read back. One whose
/// image is gone stays, refused, for the user to see.
fn read_messages(folder: &KeptFolder) -> Vec<Message> {
    let mut messages: Vec<Message> = folder.read();

    for message in &mut messages {
        if !folder.read_images(message.id, &mut message.images) {
            message.state = Stage::Failed {
                reason: "An image of this message could not be read.".into(),
            };
        } else if !folder.read_images(files_id(message.id), &mut message.files) {
            message.state = Stage::Failed {
                reason: "A file of this message could not be read.".into(),
            };
        }
    }

    messages
}

/// What a flusher needs of the machine and the windows.
pub(crate) trait Link {
    /// Sends `message`.
    async fn send(&mut self, message: &Message) -> Result<Vec<String>>;
    /// Resumes the ended session of `message`, on `message.resume`'s model.
    async fn resume(&mut self, message: &Message) -> Result<Resumed>;
    /// Applies `pick` to the session of `message`, which runs again.
    async fn configure(&mut self, message: &Message, pick: &Pick) -> Result<()>;
    /// Writes the files of `message` into its session's cache on the machine;
    /// where each went.
    async fn upload(&mut self, message: &Message) -> Result<Vec<String>>;
    /// Sends `order`.
    async fn order(&mut self, _order: &Order) -> Result<()> {
        Ok(())
    }
    /// Whether the session, read again, shows the machine has `message`.
    async fn took(&mut self, message: &Message) -> Result<bool>;
    /// Returns when the machine may answer again.
    async fn reachable(&mut self);
    /// Runs `flush_urgent` for `key` beside the flusher, which a resume or an upload holds up.
    fn urgent(&mut self, _key: &SessionKey) {}
    /// Tells the windows what the session's outbox holds now.
    fn changed(&mut self, changed: Changed);
    /// Runs after an order finished or was refused in the background.
    fn order_settled(&mut self, _order: &Order, _refused: Option<&str>) {}
}

/// The session that goes on from one that was resumed.
pub(crate) struct Resumed {
    pub(crate) session: SessionId,
    /// As the windows list it; none in tests.
    pub(crate) row: Option<SessionRow>,
}

/// What became of one try at the oldest waiting message.
enum Tried {
    Sent(Vec<String>),
    /// A resume that may have gone: its message and those behind it fail.
    Uncertain(String),
    Lost {
        unsure: bool,
    },
    Refused(String),
}

enum TriedOrder {
    Sent,
    Lost,
    Refused(String),
}

/// Sends `key`'s waiting orders and messages oldest first, until none is left.
/// One that could not reach the machine waits for it to be reachable, and
/// those after it wait behind it. One the machine refused keeps its reason
/// and the next goes.
pub(crate) async fn flush(outbox: &Outbox, key: &SessionKey, link: &mut impl Link) {
    // A message that resumes its session moves this flusher to the session it went on in.
    let mut key = key.clone();

    while let Some(step) = outbox.with(|held| held.begin_step(&key, Instant::now())) {
        match step {
            Step::Message(message) => {
                let id = message.id;

                // An urgent order kept behind this message goes beside it.
                if outbox.with(|held| held.start_urgent(&key)) {
                    link.urgent(&key);
                }

                let tried = try_message(outbox, link, &mut key, *message).await;
                let lost = matches!(tried, Tried::Lost { .. });
                let change = outbox.with(|held| match tried {
                    Tried::Sent(keys) => held.sent(id, keys),
                    Tried::Lost { unsure } => held.wait(id, unsure),
                    Tried::Refused(reason) => held.refused(id, reason),
                    Tried::Uncertain(reason) => held.uncertain(id, reason),
                });

                if let Some(change) = change {
                    link.changed(change);
                }

                if lost {
                    link.reachable().await;
                }
            }
            Step::Order(order) => run_order(outbox, link, order).await,
        }
    }
}

/// Sends `key`'s urgent orders while a resume or an upload holds up its flusher,
/// so a Stop or an answer does not wait behind it. Ends once there is none, or
/// the flusher is free again.
pub(crate) async fn flush_urgent(outbox: &Outbox, key: &SessionKey, link: &mut impl Link) {
    while let Some(order) = outbox.with(|held| held.begin_urgent(key, Instant::now())) {
        run_order(outbox, link, order).await;
    }
}

/// One try at `order`, and what the outbox and the windows make of it.
async fn run_order(outbox: &Outbox, link: &mut impl Link, order: Order) {
    let id = order.id;
    let tried = try_order(link, &order).await;
    let lost = matches!(tried, TriedOrder::Lost);
    let mut refused_bg = None;
    let change = outbox.with(|held| match tried {
        TriedOrder::Sent => held.order_sent(id),
        TriedOrder::Lost => {
            held.order_wait(id, Instant::now());
            None
        }
        TriedOrder::Refused(reason) => {
            if held.order_refused(id, reason.clone()) {
                refused_bg = Some(reason);
            }
            None
        }
    });

    if let Some(change) = change {
        link.changed(change);
    }

    if lost {
        link.reachable().await;
    } else {
        link.order_settled(&order, refused_bg.as_deref());
    }
}

async fn try_order(link: &mut impl Link, order: &Order) -> TriedOrder {
    let at_most_once = matches!(order.kind, OrderKind::Cancel | OrderKind::StopAll);

    match link.order(order).await {
        Ok(()) => TriedOrder::Sent,
        Err(DesktopError::Api(ApiError::Conflict { .. } | ApiError::NotFound {}))
            if matches!(order.kind, OrderKind::Answer { .. } | OrderKind::Close) =>
        {
            TriedOrder::Sent
        }
        // `Cancel` and `StopAll` carry no turn identity, so never replay them after a
        // dropped acknowledgement or across a reconnect where a newer turn may have started.
        Err(DesktopError::Unsure(_) | DesktopError::TimedOut(_)) if at_most_once => {
            TriedOrder::Sent
        }
        Err(e) if at_most_once => TriedOrder::Refused(e.to_string()),
        Err(e)
            if e.is_transport()
                || matches!(e, DesktopError::Unsure(_) | DesktopError::TimedOut(_)) =>
        {
            TriedOrder::Lost
        }
        Err(e) => TriedOrder::Refused(e.to_string()),
    }
}

/// What a step before the send, that did not finish, makes of the try: a
/// machine not reached, or an answer that did not come, waits and goes again;
/// what the machine refused is kept with its reason.
fn stopped(e: DesktopError) -> Tried {
    if e.is_transport() || matches!(e, DesktopError::Unsure(_) | DesktopError::TimedOut(_)) {
        Tried::Lost { unsure: false }
    } else {
        Tried::Refused(e.to_string())
    }
}

/// One try at `message`: its session resumes first if it ended, then its
/// files go, each try from the start; those already there are never uploaded
/// again, nor a session resumed twice.
async fn try_message(
    outbox: &Outbox,
    link: &mut impl Link,
    key: &mut SessionKey,
    mut message: Message,
) -> Tried {
    if let Some(resume) = message.resume.clone() {
        if !resume.resumed {
            link.changed(outbox.with(|held| held.changed(key)));

            let resumed = match link.resume(&message).await {
                Ok(resumed) => resumed,
                // Resuming again could branch a second session: not knowing fails them.
                Err(e @ (DesktopError::Unsure(_) | DesktopError::TimedOut(_))) => {
                    return Tried::Uncertain(format!(
                        "The session may have resumed ({e}). Check the session list, then send again."
                    ));
                }
                Err(e) => return stopped(e),
            };
            let Some((now, mut from, to)) =
                outbox.with(|held| held.resumed(message.id, resumed.session))
            else {
                return Tried::Refused("The message was taken back.".into());
            };
            from.resumed = resumed.row;
            from.pick = resume.pick.clone();
            link.changed(from);
            link.changed(to);
            *key = now.key();
            message = now;

            // Orders that waited for the new session may go beside what is left to do.
            if outbox.with(|held| held.start_urgent(key)) {
                link.urgent(key);
            }
        }

        if let Some(pick) = &resume.pick
            && let Err(e) = link.configure(&message, pick).await
        {
            return stopped(e);
        }

        // A Close or a Stop may have taken the message back while the session resumed.
        if !outbox.with(|held| held.resume_done(message.id)) {
            return Tried::Refused("The message was taken back.".into());
        }
    }

    // A resume of its own has nothing to send once the session runs again.
    if message.bare() {
        return Tried::Sent(Vec::new());
    }

    if message.files.is_empty() {
        return try_once(link, &message).await;
    }

    if let Some(reason) = too_big(&message.files, MAX_FILE_BYTES) {
        return Tried::Refused(reason);
    }

    link.changed(outbox.with(|held| held.changed(key)));

    let paths = match link.upload(&message).await {
        Ok(paths) => paths,
        Err(e) => return stopped(e),
    };
    let Some((message, changed)) = outbox.with(|held| held.uploaded(message.id, paths)) else {
        return Tried::Refused("The message was taken back.".into());
    };
    link.changed(changed);

    try_once(link, &message).await
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
}

impl Link for MeshLink {
    async fn send(&mut self, message: &Message) -> Result<Vec<String>> {
        let app = self.app.clone();
        let sending = sent(
            app.clone(),
            &self.window,
            app.state(),
            message.machine_id.clone(),
            message.agent.clone(),
            message.session.clone(),
            message.steer(),
        );

        logged("send", message, sending, |_| "ok").await
    }

    async fn resume(&mut self, message: &Message) -> Result<Resumed> {
        let resuming = async {
            let fleet = self.app.state::<Arc<Fleet>>();
            let request = ResumeAgentSession {
                machine_id: message.machine_id.clone(),
                agent: message.agent.clone(),
                session: message.session.clone(),
                prompt: None,
                model: message.resume.as_ref().and_then(|r| r.model.clone()),
            };
            let started = resume_session(&fleet, self.machine, request).await?;
            Ok(Resumed {
                session: started.session.id().clone(),
                row: Some(started.session),
            })
        };

        logged("resume", message, resuming, |_| "ok").await
    }

    async fn configure(&mut self, message: &Message, pick: &Pick) -> Result<()> {
        let order = Order {
            id: 0,
            key: message.key(),
            kind: OrderKind::Configure {
                model: pick.model.clone(),
                effort: pick.effort,
                fast: pick.fast,
            },
            sending: true,
            deadline: None,
        };
        let configuring = send_node_order(&self.app, self.machine, &order);

        logged("configure", message, configuring, |_| "ok").await
    }

    async fn upload(&mut self, message: &Message) -> Result<Vec<String>> {
        logged("upload", message, self.uploading(message), |_| "ok").await
    }

    async fn took(&mut self, message: &Message) -> Result<bool> {
        let checking = async {
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
        };

        logged("took", message, checking, |held| {
            if *held { "held" } else { "missing" }
        })
        .await
    }

    async fn order(&mut self, order: &Order) -> Result<()> {
        send_node_order(&self.app, self.machine, order).await
    }

    async fn reachable(&mut self) {
        let fleet = self.app.state::<Arc<Fleet>>().inner().clone();
        crate::fleet::alerts::until_online(&fleet, self.machine).await;
        sleep(RETRY).await;
    }

    fn changed(&mut self, changed: Changed) {
        emit(&self.app, changed);
    }

    fn urgent(&mut self, key: &SessionKey) {
        spawn_urgent(&self.app, key.clone(), self.window.clone());
    }

    fn order_settled(&mut self, order: &Order, refused: Option<&str>) {
        if let Some(reason) = refused {
            log::warn!(
                "background session order refused machine={} session={}: {reason}",
                order.key.machine_id,
                order.key.session
            );
            // The open session re-reads itself: no push carries a refused order, which leaves
            // the machine's session as it was.
            let _ = self
                .app
                .emit("sessions-changed", order.key.session.as_ref());
            crate::fleet::home_signal::poke(&self.app);
            return;
        }

        if matches!(order.kind, OrderKind::Close) {
            closed(&self.app, &order.key.machine_id, &order.key.session);
        }
    }
}

/// Runs one step of a try at `message` and logs it in one line: the step, the
/// session, how it ended and how long it took. Never the message's text, its
/// files' names or paths, nor the error's words, which can carry them.
async fn logged<T>(
    step: &str,
    message: &Message,
    run: impl Future<Output = Result<T>>,
    outcome: impl FnOnce(&T) -> &'static str,
) -> Result<T> {
    let started = Instant::now();
    let result = run.await;
    let ended = match &result {
        Ok(value) => outcome(value),
        Err(e) => failure(e),
    };
    log::info!(
        "outbox try step={step} machine={} session={} outcome={ended} took_ms={}",
        message.machine_id,
        message.session,
        started.elapsed().as_millis()
    );

    result
}

/// How a step failed, in one word.
fn failure(e: &DesktopError) -> &'static str {
    match e {
        e if e.is_transport() => "lost",
        DesktopError::Unsure(_) => "unsure",
        DesktopError::TimedOut(_) => "timed_out",
        DesktopError::Api(ApiError::NotFound {}) => "not_found",
        _ => "refused",
    }
}

impl MeshLink {
    /// Writes the files of `message` into its session's cache on the machine.
    async fn uploading(&self, message: &Message) -> Result<Vec<String>> {
        let fleet = self.app.state::<Arc<Fleet>>();
        let mut paths = Vec::new();

        for file in &message.files {
            let size = base64_size(&file.data);
            let text = file.data.as_bytes();
            // The bytes are decoded a part at a time, never the whole file at once.
            let mut parts: Vec<&[u8]> = text.chunks(PART_CHARS).collect();
            parts.resize(parts.len().max(1), &[]);
            // The machine names the file after its first part.
            let mut path = file.name.clone();
            let mut offset = 0u64;

            for part in parts {
                // A message taken back, with its session closed, uploads no further.
                if !self
                    .app
                    .state::<Arc<Outbox>>()
                    .read(|held| held.holds(message.id))
                {
                    return Err(DesktopError::BadRequest(
                        "The message was taken back.".into(),
                    ));
                }

                let data = STANDARD.decode(part).map_err(|_| {
                    DesktopError::BadRequest(format!("{} could not be read", file.name))
                })?;
                let len = u64::try_from(data.len()).map_err(anyhow::Error::from)?;
                let write = WriteWorkFile {
                    machine_id: message.machine_id.clone(),
                    root: message.root.clone(),
                    path,
                    offset,
                    size,
                    data,
                };
                offset += len;
                path = crate::fleet::files::upload(
                    &fleet,
                    self.machine,
                    message.agent.clone(),
                    message.session.clone(),
                    write,
                )
                .await?;
            }

            paths.push(path);
        }

        Ok(paths)
    }
}

async fn send_node_order(app: &AppHandle, machine: EndpointId, order: &Order) -> Result<()> {
    let fleet = app.state::<Arc<Fleet>>();

    if let OrderKind::Answer { question, reply } = &order.kind {
        let operation = Operation::Agent {
            agent: order.key.agent.clone(),
            session: order.key.session.clone(),
            request: AgentRequest::Answer {
                question: *question,
                reply: reply.clone(),
            },
        };

        if let Some(ack) = app
            .state::<Events>()
            .call_machine(machine, operation, ORDER_LIMIT)
            .await?
        {
            let CallReply::Agent { .. } = ack else {
                return Err(DesktopError::BadRequest(
                    "unexpected session acknowledgement".into(),
                ));
            };
            return Ok(());
        }

        let notice = AgentNoticeKey {
            machine_id: order.key.machine_id.clone(),
            agent: order.key.agent.clone(),
            session: order.key.session.clone(),
            question: Some(*question),
            machine_name: String::new(),
        };
        return alerts::answer(&fleet, &notice, reply).await;
    }

    let version = fleet.status().version(machine);
    let steer = match &order.kind {
        OrderKind::Configure {
            model,
            effort,
            fast: None,
        } => AgentSteer::Configure {
            model: model.clone(),
            effort: *effort,
        },
        OrderKind::Configure {
            model,
            effort,
            fast: Some(fast),
        } => AgentSteer::ConfigureFast {
            model: model.clone(),
            effort: *effort,
            fast: *fast,
        },
        OrderKind::Mode { mode } => AgentSteer::SetMode { mode: mode.clone() },
        OrderKind::Device { device } => AgentSteer::SetDevice {
            device: device.clone(),
        },
        OrderKind::Cancel => AgentSteer::Cancel {},
        OrderKind::StopAll if newer_than(version.as_deref(), &BEFORE_STOP_ALL) => {
            AgentSteer::StopAll {}
        }
        OrderKind::StopAll => AgentSteer::Cancel {},
        OrderKind::Close => AgentSteer::Close {},
        OrderKind::StopTask { task } => AgentSteer::StopTask { task: task.clone() },
        OrderKind::Answer { .. } => unreachable!(),
    };
    let close = matches!(steer, AgentSteer::Close {});
    let operation = Operation::Agent {
        agent: order.key.agent.clone(),
        session: order.key.session.clone(),
        request: beep_mesh::machine::node_request(steer.clone())?,
    };

    if let Some(ack) = app
        .state::<Events>()
        .call_machine(machine, operation, ORDER_LIMIT)
        .await?
    {
        let CallReply::Agent { .. } = ack else {
            return Err(DesktopError::BadRequest(
                "unexpected session acknowledgement".into(),
            ));
        };
        return Ok(());
    }

    let direct_order = SteerAgentSession {
        machine_id: order.key.machine_id.clone(),
        agent: order.key.agent.clone(),
        session: order.key.session.clone(),
        steer,
    };
    let sent = direct::steer(&fleet, machine, direct_order);
    let opened = fleet
        .asked(
            machine,
            &order.key.session,
            "sending to the session",
            ORDER_LIMIT,
            sent,
        )
        .await?;

    if close {
        let fleet = fleet.inner().clone();
        let app = app.clone();
        let machine_id = order.key.machine_id.clone();
        let session = order.key.session.clone();
        tauri::async_runtime::spawn(async move {
            direct::until_ended(&fleet, machine, opened).await;
            closed(&app, &machine_id, &session);
        });
    }

    Ok(())
}

/// Clears a session's answered question locally at once so Home, notifications, and the tray
/// do not stay stuck waiting on a slow node link. The one place an answer is applied: the
/// order's later settling changes nothing more.
fn answered_locally(app: &AppHandle, key: &AgentNoticeKey) {
    crate::notices::history::answered(app, key);

    if let Some(mirror) = app.try_state::<Arc<SessionMirror>>() {
        mirror.answered(&key.machine_id, &key.session, key.question);
    }
}

/// The part of a session read back after a dropped send, or pushed by its follow.
pub(crate) struct Log<'a> {
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

/// A push of `key`'s session lists these messages in its queue and its log: the sent rows it
/// holds go. Called for each update a follow receives, before it is handed to the window.
pub(crate) fn settled(
    app: &AppHandle,
    key: &SessionKey,
    queued: &[String],
    earlier: u64,
    entries: &[AgentEntry],
) {
    let log = Log {
        queued,
        earlier,
        entries,
    };

    if let Some(changed) = app
        .state::<Arc<Outbox>>()
        .with(|held| held.settled(key, &log))
    {
        emit(app, changed);
    }
}

/// The machine dropped the steers it held for `key`: the sent rows of the session go with them.
pub(crate) fn dropped(app: &AppHandle, key: &SessionKey) {
    if let Some(changed) = app.state::<Arc<Outbox>>().with(|held| held.dropped(key)) {
        emit(app, changed);
    }
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
        };
        flush(&outbox, &key, &mut link).await;
    });
}

/// `key`, or the session it went on in if it was resumed, so an order or a read
/// meant for the ended one reaches the one that runs.
pub(crate) fn resolved(app: &AppHandle, key: &SessionKey) -> SessionKey {
    app.state::<Arc<Outbox>>().read(|held| held.resolve(key))
}

/// Runs `key`'s urgent lane, which the caller marked as running.
fn spawn_urgent(app: &AppHandle, key: SessionKey, window: String) {
    let Some(machine) = app.state::<Arc<Fleet>>().member(&key.machine_id) else {
        app.state::<Arc<Outbox>>()
            .with(|held| held.urgent.remove(&key));
        return;
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let outbox = app.state::<Arc<Outbox>>().inner().clone();
        let mut link = MeshLink {
            app: app.clone(),
            window,
            machine,
        };
        flush_urgent(&outbox, &key, &mut link).await;
    });
}

/// Starts a flusher for each session with messages kept from the last run.
pub fn resume(app: &AppHandle) {
    let outbox = app.state::<Arc<Outbox>>();
    let keys = outbox.with(|held| held.waiting());

    if keys.is_empty() {
        return;
    }

    // Setup runs while the mesh opens, and `start` needs its members.
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let fleet = app.state::<Arc<Fleet>>().inner().clone();
        // A mesh that fails to open leaves no members: `start` drops the flushers.
        let _ = fleet.ready().await;

        // Marked only now, so a send while the mesh opens still starts its own flusher.
        let outbox = app.state::<Arc<Outbox>>().inner().clone();
        for key in keys {
            if outbox.with(|held| held.flushing.insert(key.clone())) {
                start(&app, key, String::new());
            }
        }
    });
}

/// Keeps a message for `key`'s session in the outbox, after the ones waiting there, and
/// tells the windows its row. Every message is the outbox's from the start: its row says what
/// is done, and the window waits on nothing. Always `Waiting` for the caller, unless the
/// steer is not a message.
pub(crate) fn send(
    app: &AppHandle,
    window: &str,
    key: SessionKey,
    steer: Steer,
    seen: Option<Seen>,
    attach: Attach,
    resume: Option<Resume>,
) -> SteerError {
    let (how, text, images) = match steer {
        Steer::Prompt { text, images } => (Kind::Prompt, text, images),
        Steer::InTurn { text, images } => (Kind::Steer, text, images),
        _ => {
            return SteerError::Failed {
                message: "only messages wait for their machine".into(),
            };
        }
    };
    let draft = Draft {
        how,
        text,
        images,
        files: attach.files.into_iter().map(Into::into).collect(),
        root: attach.root,
        mentions: attach.mentions,
        resume,
        seen,
        reading: attach.reading,
    };
    let outbox = app.state::<Arc<Outbox>>();
    let (key, flusher, changed) = outbox.with(|held| {
        let (_, flusher) = held.push(&key, draft);
        let key = held.resolve(&key);
        let changed = held.changed(&key);
        (key, flusher, changed)
    });
    emit(app, changed);

    if flusher {
        start(app, key, window.to_string());
    }

    SteerError::Waiting {
        message: "Waiting for the connection".into(),
    }
}

/// Submits a session control order to `key`'s actor: returns immediately once a fast node
/// acknowledges or refuses, or hands off to the background actor after `ANSWER_WAIT` when a
/// node link is degraded.
pub(crate) async fn send_order(
    app: &AppHandle,
    window: &str,
    key: SessionKey,
    kind: OrderKind,
) -> Result<OrderAnswer> {
    let (reply, mut answer) = oneshot::channel();
    let outbox = app.state::<Arc<Outbox>>();
    let (id, changed, flusher, urgent) = outbox.with(|held| {
        let (id, changed, flusher) = held.push_order(&key, kind, Instant::now(), Some(reply));
        (id, changed, flusher, held.start_urgent(&key))
    });

    if let Some(changed) = changed {
        emit(app, changed);
    }

    if urgent {
        spawn_urgent(app, key.clone(), window.to_string());
    }

    if flusher {
        start(app, key, window.to_string());
    }

    let outcome = match timeout(ANSWER_WAIT, &mut answer).await {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(_)) => OrderAnswer::Queued,
        Err(_) => {
            outbox.with(|held| held.detach_order(id));
            answer.try_recv().unwrap_or(OrderAnswer::Queued)
        }
    };

    match outcome {
        OrderAnswer::Refused(message) => Err(DesktopError::BadRequest(message)),
        other => Ok(other),
    }
}

/// Answers a coding-agent question through the session's actor, clearing local prompt state
/// at once when accepted or queued behind a degraded link.
pub(crate) async fn answer(
    app: &AppHandle,
    fleet: &Arc<Fleet>,
    window: &str,
    key: &AgentNoticeKey,
    reply: String,
) -> Result<()> {
    let Some(question) = key.question else {
        return Ok(());
    };

    if !alerts::reaches(fleet, &key.machine_id) {
        return Err(DesktopError::NotInMesh);
    }

    let session_key = SessionKey {
        machine_id: key.machine_id.clone(),
        agent: key.agent.clone(),
        session: key.session.clone(),
    };
    send_order(
        app,
        window,
        session_key,
        OrderKind::Answer { question, reply },
    )
    .await?;
    answered_locally(app, key);
    Ok(())
}

/// The messages waiting on this device for a session.
#[tauri::command]
pub fn outbox_list(
    outbox: State<'_, Arc<Outbox>>,
    machine_id: String,
    agent: String,
    session: SessionId,
) -> Listed {
    let key = SessionKey {
        machine_id,
        agent,
        session,
    };
    outbox.read(|held| held.listed(&key))
}

/// A waiting message as the field takes it back.
#[derive(Debug, Serialize)]
pub struct Taken {
    text: String,
    images: Vec<PastedImage>,
    files: Vec<PastedImage>,
    mentions: Vec<String>,
}

/// Takes a waiting message out of the outbox, to remove or edit it.
#[tauri::command]
pub fn outbox_take(app: AppHandle, outbox: State<'_, Arc<Outbox>>, id: u64) -> Result<Taken> {
    let (message, changed) = outbox.with(|held| held.take(id))?;
    emit(&app, changed);
    Ok(message.taken())
}

/// Completes the message handed over with its files unread: the words and files the window has
/// now. False when the message is gone, taken back meanwhile.
#[tauri::command]
pub fn outbox_attach(app: AppHandle, window: WebviewWindow, token: String, read: Read) -> bool {
    let Some((changed, key, flusher)) = app
        .state::<Arc<Outbox>>()
        .with(|held| held.attach(&token, read))
    else {
        return false;
    };
    emit(&app, changed);

    if flusher {
        start(&app, key, window.label().to_string());
    }

    true
}

/// The window could not read the files of the message under `token`: it is taken out, and the
/// window puts it back in the field.
#[tauri::command]
pub fn outbox_cancel(app: AppHandle, outbox: State<'_, Arc<Outbox>>, token: String) {
    if let Some(changed) = outbox.with(|held| held.cancel(&token)) {
        emit(&app, changed);
    }
}

/// Takes every waiting message of a session out at once, oldest first, as
/// Stop all puts them back in the field. One being sent is left to go.
#[tauri::command]
pub fn outbox_drain(
    app: AppHandle,
    outbox: State<'_, Arc<Outbox>>,
    machine_id: String,
    agent: String,
    session: SessionId,
) -> Vec<Taken> {
    let key = SessionKey {
        machine_id,
        agent,
        session,
    };
    let (messages, changed) = outbox.with(|held| held.drain(&held.resolve(&key)));
    emit(&app, changed);
    messages.into_iter().map(Message::taken).collect()
}

#[cfg(test)]
#[path = "../tests/agents_outbox.rs"]
mod tests;
