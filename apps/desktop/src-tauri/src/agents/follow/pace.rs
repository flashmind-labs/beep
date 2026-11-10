//! Paces the updates of a session followed off screen: at most one a second,
//! the newest state with every entry kept, and at once when its run starts or
//! ends, it asks something, or it shows again. While its window is hidden
//! everything is held, and the newest state goes once the window shows.

use super::UpdateKind;
use crate::agents::sessions::{ModesRow, TaskRow};
use beep_proto::{AgentSessionView, AgentStatus, node::agent::SessionCursor};
use std::time::{Duration, Instant};

/// How often a session off screen goes to the webview, at most.
const OFF_SCREEN_EVERY: Duration = Duration::from_secs(1);

/// One update for the webview, before its view becomes a page.
pub(super) struct Pushed {
    pub(super) kind: UpdateKind,
    pub(super) cursor: SessionCursor,
    pub(super) resumed: bool,
    pub(super) view: AgentSessionView,
    pub(super) modes: Option<ModesRow>,
    pub(super) tasks: Vec<TaskRow>,
}

/// Whether a session's run goes on and what it asks: a change goes at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Turn {
    active: bool,
    question: Option<u32>,
}

impl From<&AgentSessionView> for Turn {
    fn from(view: &AgentSessionView) -> Self {
        let active = matches!(
            view.session.status,
            AgentStatus::Starting {}
                | AgentStatus::Working {}
                | AgentStatus::Asking {}
                | AgentStatus::Running {}
        );
        Self {
            active,
            question: view.session.question.as_ref().map(|question| question.id),
        }
    }
}

impl Pushed {
    /// Joins `next` on after these entries, as the webview would, or gives
    /// it back when it does not continue them.
    fn join(&mut self, next: Pushed) -> Option<Pushed> {
        if matches!(next.kind, UpdateKind::Snapshot) && !next.resumed {
            *self = next;
            return None;
        }

        let held = u64::from(self.view.earlier);
        let at = u64::from(next.view.earlier);
        let end = held + self.view.entries.len() as u64;

        if at < held || at > end {
            return Some(next);
        }

        let Ok(keep) = usize::try_from(at - held) else {
            return Some(next);
        };

        let mut entries = std::mem::take(&mut self.view.entries);
        entries.truncate(keep);
        entries.extend(next.view.entries);
        self.view = AgentSessionView {
            entries,
            earlier: self.view.earlier,
            ..next.view
        };
        self.cursor = next.cursor;
        self.modes = next.modes;
        self.tasks = next.tasks;
        None
    }
}

#[derive(Default)]
pub(super) struct Pace {
    held: Option<Pushed>,
    sent: Option<Instant>,
    turn: Option<Turn>,
    /// The window is hidden: nothing is due.
    hidden: bool,
}

impl Pace {
    /// Takes `next` at `now`; what goes to the webview now, oldest first.
    /// The session on screen, when `shown`, gets every update at once.
    pub(super) fn push(&mut self, next: Pushed, shown: bool, now: Instant) -> Vec<Pushed> {
        let turn = Turn::from(&next.view);
        let urgent = !self.hidden && (shown || self.turn != Some(turn));
        self.turn = Some(turn);
        let mut out = Vec::new();
        let rest = match &mut self.held {
            Some(held) => held.join(next),
            None => Some(next),
        };

        if let Some(rest) = rest {
            out.extend(self.held.replace(rest));
        }

        let quiet = !self.hidden
            && self
                .sent
                .is_none_or(|sent| now.saturating_duration_since(sent) >= OFF_SCREEN_EVERY);

        if urgent || quiet {
            out.extend(self.held.take());
        }

        if !out.is_empty() {
            self.sent = Some(now);
        }

        out
    }

    /// When what is held is due.
    pub(super) fn due(&self) -> Option<Instant> {
        if self.hidden {
            return None;
        }

        self.held.as_ref()?;
        self.sent.map(|sent| sent + OFF_SCREEN_EVERY)
    }

    /// The window hid or showed again at `now`; on show, what is held.
    pub(super) fn window(&mut self, hidden: bool, now: Instant) -> Option<Pushed> {
        self.hidden = hidden;

        if hidden { None } else { self.flush(now) }
    }

    /// What is held, sent at `now`.
    pub(super) fn flush(&mut self, now: Instant) -> Option<Pushed> {
        let held = self.held.take()?;
        self.sent = Some(now);
        Some(held)
    }
}

#[cfg(test)]
#[path = "../../tests/agents_follow_pace.rs"]
mod tests;
