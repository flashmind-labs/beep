//! Notices for a user's devices and the messages that start a turn.

use crate::{
    protocol::{ApprovalDecision, DeviceKind, Notice, Platform, Upload},
    routines::wake::Woken,
};
use flashmind::types::{Model, ReasoningLevel};
use iroh::EndpointId;
use uuid::Uuid;

/// A `Notice` addressed to one user's open notification streams.
#[derive(Debug, Clone)]
pub struct UserNotice {
    pub user: String,
    pub notice: Notice,
    /// What the user's computers get instead of `notice`, when it differs.
    pub desktop: Option<Notice>,
    /// Which of the user's devices get it.
    pub to: Reach,
}

/// Which of a user's devices a `UserNotice` goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    Everyone,
    /// The computers only, never a phone.
    Desktops,
    /// Only the device with this key, such as one that started a coding-agent session.
    Device(EndpointId),
}

impl Reach {
    pub(super) fn includes(self, id: &EndpointId, kind: &DeviceKind) -> bool {
        match self {
            Self::Everyone => true,
            Self::Desktops => matches!(kind, DeviceKind::Desktop {}),
            Self::Device(device) => device == *id,
        }
    }
}

impl UserNotice {
    /// Tells the user's phones `session_id` changed, or the chat list when empty.
    pub fn changed(user: &str, session_id: &str) -> Self {
        Self {
            user: user.to_owned(),
            notice: Notice {
                title: String::new(),
                body: String::new(),
                session_id: session_id.to_owned(),
                app: None,
                approval: None,
                changed: true,
                group: false,
            },
            desktop: None,
            to: Reach::Everyone,
        }
    }

    /// The notice device `id` of `kind` gets; `None` when the notice is not for it.
    pub fn for_device(&self, id: &EndpointId, kind: &DeviceKind) -> Option<&Notice> {
        if !self.to.includes(id, kind) {
            return None;
        }
        Some(match &self.desktop {
            Some(desktop) if matches!(kind, DeviceKind::Desktop {}) => desktop,
            _ => &self.notice,
        })
    }

    /// Tells the user's devices that group `id` changed.
    pub fn group_changed(user: &str, id: &str) -> Self {
        let mut notice = Self::changed(user, id);
        notice.notice.group = true;
        notice
    }
}

pub struct Message {
    pub session_id: Uuid,
    pub request_id: Uuid,
    pub message: String,
    pub cron_id: Option<String>,
    /// A routine's label, empty when it has none. Set for routine runs only.
    pub scheduled: Option<String>,
    /// A routine whose reply always reaches the user; set for the evening digest only.
    pub always_sent: bool,
    /// An event's label when an event woke the agent (see `crate::routines::wake`).
    pub woken: Option<Woken>,
    pub images: Vec<String>,
    /// Other files sent with the message, such as a PDF.
    pub files: Vec<Upload>,
    /// The recording a held reply came as; `message` holds its transcript.
    pub voice: Option<Upload>,
    /// Said aloud in live mode; `message` is what was heard.
    pub spoken: bool,
    /// Set when the run resumes a session that stopped to ask the user.
    pub answer: Option<Answer>,
    /// A one-off model for this run, instead of the user's picked one.
    pub model: Option<Model>,
    /// A one-off thinking for this run, instead of the user's picked one.
    pub thinking: Option<ReasoningLevel>,
    /// The device the message came from. A computer has no phone to act on.
    pub origin: DeviceKind,
    /// The app the message came from, when it says.
    pub platform: Option<Platform>,
    /// What the app draws under a reply, so the run gets those tools.
    pub draws: Draws,
    /// The lead's background turn telling the user a research ended.
    pub finished_research: bool,
}

impl Message {
    /// Something the user typed, not a routine, event or card answer.
    pub(crate) fn is_prompt(&self) -> bool {
        self.cron_id.is_none()
            && self.scheduled.is_none()
            && self.woken.is_none()
            && self.answer.is_none()
            && !self.finished_research
    }

    /// A prompt said aloud, live or as a held reply.
    pub(super) fn is_spoken(&self) -> bool {
        self.is_prompt() && (self.spoken || self.voice.is_some())
    }
}

/// What an app draws under a reply; a run only gets tools whose results its app shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Draws {
    /// `show_chart` and `show_stock_chart`.
    pub charts: bool,
    /// `show_cards`.
    pub cards: bool,
    /// Source chips: the app shows the web pages a reply used, so the model need not list them.
    pub sources: bool,
}

/// The user's answer to the tool call a session stopped on.
#[derive(Debug, Clone)]
pub struct Answer {
    pub call_id: String,
    pub decision: ApprovalDecision,
    /// With `Allow`, the action also runs without asking for the rest of the chat.
    pub for_session: bool,
}

impl Message {
    /// The run's request id, keyed by the call so answering twice returns the same run.
    pub(crate) fn request(&self) -> String {
        match &self.answer {
            Some(answer) => format!("approval:{}", answer.call_id),
            None => self.request_id.to_string(),
        }
    }
}
