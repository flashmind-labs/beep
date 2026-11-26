//! Paces the updates of a session followed off screen: at most one a second,
//! the newest state with every entry kept, and at once when its run starts or
//! ends, it asks something, or it shows again. While its window is hidden
//! everything is held, and the newest state goes once the window shows.

use crate::agents::sessions::ModesRow;
use beep_proto::{AgentSessionView, AgentStatus, AgentTask, node::agent::SessionCursor};
use serde::Serialize;
use std::time::{Duration, Instant};

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum UpdateKind {
    Snapshot,
    Delta,
}

/// How often a session off screen goes to the webview, at most.
const OFF_SCREEN_EVERY: Duration = Duration::from_secs(1);

/// One update for the webview, before its view becomes a page.
pub(super) struct Pushed {
    pub(super) kind: UpdateKind,
    pub(super) cursor: SessionCursor,
    pub(super) resumed: bool,
    pub(super) view: AgentSessionView,
    pub(super) modes: Option<ModesRow>,
    pub(super) tasks: Vec<AgentTask>,
}

/// Whether a session's run goes on and what it asks: a change goes at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Turn {
    active: bool,
    question: Option<u32>,
}

/// Whether a session's run goes on.
pub(super) fn running(view: &AgentSessionView) -> bool {
    matches!(
        view.session.status,
        AgentStatus::Starting {}
            | AgentStatus::Working {}
            | AgentStatus::Asking {}
            | AgentStatus::Running {}
    )
}

impl From<&AgentSessionView> for Turn {
    fn from(view: &AgentSessionView) -> Self {
        Self {
            active: running(view),
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

        let Pushed {
            kind,
            cursor,
            resumed,
            view,
            modes,
            tasks,
        } = next;

        if let Some(view) = join_view(&mut self.view, view, cursor.start) {
            return Some(Pushed {
                kind,
                cursor,
                resumed,
                view,
                modes,
                tasks,
            });
        }

        self.cursor = cursor;
        self.modes = modes;
        self.tasks = tasks;
        None
    }
}

/// Joins `next` on after `held`'s entries, as the webview would, and drops
/// those before `start`, the cursor's first, as the webview drops them, so a
/// long run held off screen does not grow. Gives `next` back when it does not
/// continue `held`.
pub(super) fn join_view(
    held: &mut AgentSessionView,
    next: AgentSessionView,
    start: u64,
) -> Option<AgentSessionView> {
    let from = u64::from(held.earlier);
    let at = u64::from(next.earlier);
    let end = from + held.entries.len() as u64;

    if at < from || at > end {
        return Some(next);
    }

    let Ok(keep) = usize::try_from(at - from) else {
        return Some(next);
    };

    let mut entries = std::mem::take(&mut held.entries);
    entries.truncate(keep);
    entries.extend(next.entries);

    let cut = start.saturating_sub(from).min(entries.len() as u64);
    let (cut, earlier) = match (usize::try_from(cut), u32::try_from(from + cut)) {
        (Ok(cut), Ok(earlier)) => (cut, earlier),
        _ => (0, held.earlier),
    };
    entries.drain(..cut);
    *held = AgentSessionView {
        entries,
        earlier,
        ..next
    };
    None
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
