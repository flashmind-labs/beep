//! The approvals waiting while live mode is on: the live reply's own, and
//! any background work or other chat asks meanwhile. The orb shows them;
//! one still waiting when live mode ends shows as its notification.

use super::Waiting;
use beep_proto::Notice;

/// One approval, and the notice that asked it, held back while live mode
/// shows it.
struct Ask {
    waiting: Waiting,
    notice: Option<Notice>,
}

#[derive(Default)]
pub(super) struct Asks(Vec<Ask>);

impl Asks {
    /// Adds `waiting`, or gives the one already there its notice. True when
    /// it is new, so the orb should hear of it.
    pub(super) fn add(&mut self, waiting: Waiting, notice: Option<Notice>) -> bool {
        match self.find(&waiting.approval.query_id) {
            Some(at) => {
                let ask = &mut self.0[at];
                if ask.notice.is_none() {
                    ask.notice = notice;
                }
                false
            }
            None => {
                self.0.push(Ask { waiting, notice });
                true
            }
        }
    }

    /// Takes approval `query_id` away, answered or no longer waiting.
    pub(super) fn remove(&mut self, query_id: &str) -> Option<Waiting> {
        let at = self.find(query_id)?;
        Some(self.0.remove(at).waiting)
    }

    /// Takes away the approvals the turns of chat `session` wait on: a new
    /// message there leaves them unanswered. Background work still waits.
    pub(super) fn drop_turns_of(&mut self, session: &str) -> Vec<String> {
        let (dropped, kept) = std::mem::take(&mut self.0)
            .into_iter()
            .partition(|a| !a.waiting.work && a.waiting.session == session);
        self.0 = kept;
        dropped
            .into_iter()
            .map(|a: Ask| a.waiting.approval.query_id)
            .collect()
    }

    /// Every approval waiting, oldest first.
    pub(super) fn all(&self) -> Vec<Waiting> {
        self.0.iter().map(|a| a.waiting.clone()).collect()
    }

    /// The notices of the approvals still waiting, for when live mode ends.
    pub(super) fn into_notices(self) -> Vec<Notice> {
        self.0.into_iter().filter_map(|a| a.notice).collect()
    }

    fn find(&self, query_id: &str) -> Option<usize> {
        self.0
            .iter()
            .position(|a| a.waiting.approval.query_id == query_id)
    }
}

#[cfg(test)]
#[path = "../tests/voice_asks.rs"]
mod tests;
