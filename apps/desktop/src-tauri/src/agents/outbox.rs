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
        sessions::closed,
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
use beep_mesh::RETRY;
use beep_proto::SessionId;
use beep_proto::{
    AgentEntry, AgentEntryKind, AgentNoticeKey, AgentSteer, ApiError, GetAgentSession, ModelId,
    SteerAgentSession,
    node::agent::{Effort, Request as AgentRequest, SessionDevice},
    node::events::{CallReply, Operation, Topic},
};
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
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
/// How long a send or session order waits for a healthy local/fast node before handing
/// off to the session's background actor so a degraded link never stalls the UI.
const ANSWER_WAIT: Duration = Duration::from_millis(250);
/// How long transient interrupt orders (`Cancel`, `StopAll`, `StopTask`) stay queued if
/// a machine remains unreachable.
const ORDER_TTL: Duration = Duration::from_secs(60);

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
    pub(crate) session: SessionId,
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
    pub(crate) session: SessionId,
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
    Message(Message),
}

/// The kept messages, transient session orders, and what is being done with them.
#[derive(Default)]
pub(crate) struct Held {
    messages: Vec<Message>,
    orders: Vec<Order>,
    next: u64,
    /// Sessions whose flusher runs.
    flushing: HashSet<SessionKey>,
    /// The windows waiting on a message's first try. A message with one is
    /// not shown in the outbox: the window shows it as sending.
    replies: HashMap<u64, oneshot::Sender<Answer>>,
    /// Callers waiting briefly on an order's first try.
    order_replies: HashMap<u64, oneshot::Sender<OrderAnswer>>,
    /// Folder changes not yet handed to the writer, and whether the
    /// messages' file changed.
    writes: Vec<Write>,
    dirty: bool,
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

        self.writes.extend(image_writes(id, &images));

        let stuck = self.is_stuck(key);
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

    fn is_stuck(&self, key: &SessionKey) -> bool {
        self.messages.iter().any(|m| {
            m.key() == *key && m.state == Stage::Waiting && !self.replies.contains_key(&m.id)
        }) || self
            .orders
            .iter()
            .any(|o| o.key == *key && !o.sending && !self.order_replies.contains_key(&o.id))
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
        let expired: Vec<u64> = self
            .orders
            .iter()
            .filter(|o| o.key == *key && !o.sending && o.deadline.is_some_and(|d| now >= d))
            .map(|o| o.id)
            .collect();

        for id in expired {
            self.remove_order(id, OrderAnswer::Queued);
        }

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
        let msg_id = self
            .messages
            .iter()
            .find(|m| m.key() == *key && m.state == Stage::Waiting)
            .map(|m| m.id);

        match (order_id, msg_id) {
            (Some(oid), Some(mid)) if oid < mid => self.begin_order(oid).map(Step::Order),
            (Some(oid), None) => self.begin_order(oid).map(Step::Order),
            (_, Some(_)) => self.begin(key).map(Step::Message),
            (None, None) => {
                self.flushing.remove(key);
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
        let close_key = self
            .orders
            .iter()
            .find(|o| o.id == id && matches!(o.kind, OrderKind::Close))
            .map(|o| o.key.clone());
        self.remove_order(id, OrderAnswer::Sent);

        if let Some(key) = close_key {
            let (drained, changed) = self.drain(&key);
            if !drained.is_empty() {
                return Some(changed);
            }
        }

        None
    }

    pub(crate) fn order_wait(&mut self, id: u64, now: Instant) -> Option<Changed> {
        let key = self.orders.iter().find(|o| o.id == id)?.key.clone();

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

        let msg_ids: Vec<u64> = self
            .messages
            .iter()
            .filter(|m| m.key() == key)
            .map(|m| m.id)
            .collect();
        let mut handed_message = false;

        for mid in msg_ids {
            if let Some(waiter) = self.replies.remove(&mid) {
                let _ = waiter.send(Answer::Waiting);
                handed_message = true;
            }
        }

        handed_message.then(|| self.changed(&key))
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

        self.writes.extend(forget_images(id, message.images.len()));

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
    /// under the lock so no waiting image is copied.
    pub(crate) fn with<T>(&self, change: impl FnOnce(&mut Held) -> T) -> T {
        let (out, writes) = {
            let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
            let out = change(&mut held);
            let file = std::mem::take(&mut held.dirty)
                .then(|| self.folder.records(&held.messages))
                .flatten();
            let writes: Vec<Write> = std::mem::take(&mut held.writes)
                .into_iter()
                .chain(file)
                .collect();
            (out, writes)
        };

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

/// The messages kept in `folder`, each with its images read back. One whose
/// image is gone stays, refused, for the user to see.
fn read_messages(folder: &KeptFolder) -> Vec<Message> {
    let mut messages: Vec<Message> = folder.read();

    for message in &mut messages {
        if !folder.read_images(message.id, &mut message.images) {
            message.state = Stage::Failed {
                reason: "An image of this message could not be read.".into(),
            };
        }
    }

    messages
}

/// What a flusher needs of the machine and the windows.
pub(crate) trait Link {
    /// Sends `message`.
    async fn send(&mut self, message: &Message) -> Result<Vec<String>>;
    /// Sends `order`.
    async fn order(&mut self, _order: &Order) -> Result<()> {
        Ok(())
    }
    /// Whether the session, read again, shows the machine has `message`.
    async fn took(&mut self, message: &Message) -> Result<bool>;
    /// Returns when the machine may answer again.
    async fn reachable(&mut self);
    /// Tells the windows what the session's outbox holds now.
    fn changed(&mut self, changed: Changed);
    /// Runs after an order finished or was refused in the background.
    fn order_settled(&mut self, _order: &Order, _refused: Option<&str>) {}
}

/// What became of one try at the oldest waiting message.
enum Tried {
    Sent(Vec<String>),
    Lost { unsure: bool },
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
    while let Some(step) = outbox.with(|held| held.begin_step(key, Instant::now())) {
        match step {
            Step::Message(message) => {
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
            Step::Order(order) => {
                let id = order.id;
                let tried = try_order(link, &order).await;
                let lost = matches!(tried, TriedOrder::Lost);
                let mut refused_bg = None;
                let change = outbox.with(|held| match tried {
                    TriedOrder::Sent => held.order_sent(id),
                    TriedOrder::Lost => held.order_wait(id, Instant::now()),
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
        }
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
        let keys = sent(
            app.clone(),
            &self.window,
            app.state(),
            message.machine_id.clone(),
            message.agent.clone(),
            message.session.clone(),
            message.steer(),
        )
        .await?;
        Ok(keys)
    }

    async fn order(&mut self, order: &Order) -> Result<()> {
        send_node_order(&self.app, self.machine, order).await
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
        sleep(RETRY).await;
    }

    fn changed(&mut self, changed: Changed) {
        emit(&self.app, changed);
    }

    fn order_settled(&mut self, order: &Order, refused: Option<&str>) {
        if let Some(reason) = refused {
            log::warn!(
                "background session order refused machine={} session={}: {reason}",
                order.key.machine_id,
                order.key.session
            );
            let _ = self
                .app
                .emit("sessions-changed", order.key.session.as_ref());
            crate::fleet::event_views::sessions_changed(self.app.clone(), Topic::new("agents"));
            crate::fleet::event_views::home_changed(self.app.clone());
            return;
        }

        if matches!(order.kind, OrderKind::Close) {
            closed(&self.app, &order.key.machine_id, &order.key.session);
        }
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
        };
        flush(&outbox, &key, &mut link).await;
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
    let (id, changed, flusher) =
        outbox.with(|held| held.push_order(&key, kind, Instant::now(), Some(reply)));

    if let Some(changed) = changed {
        emit(app, changed);
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
    session: SessionId,
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
