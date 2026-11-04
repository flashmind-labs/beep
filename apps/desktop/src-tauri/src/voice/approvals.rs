//! Approvals while live mode is on: shown in the orb, said aloud when it is
//! quiet, and answered from the bar or the orb.

use super::{
    Voice,
    event::LiveEvent,
    news::{self, Teller},
};
use crate::{
    bar,
    beep::{Asking, Decision, Field},
};
use anyhow::Context;
use beep_live::{Input, Phase};
use beep_proto::{Notice, Say};
use serde::Serialize;
use std::sync::Arc;

/// What a toast says when an approval comes while live mode is on.
const NEEDS_OK: &str = "Beep needs your OK";

/// An approval the user answered, and the run the answer resumed.
pub struct Answered {
    pub session: String,
    pub query_id: String,
    pub run: String,
    pub decision: Decision,
}

/// An action waiting for the user's OK while live mode is on, and the chat
/// it is in: the live reply's, or one from background work or another chat.
#[derive(Debug, Clone, Serialize)]
pub struct Waiting {
    pub(super) session: String,
    pub(super) approval: Asking,
    /// Background work asked it: answered with `answer_work`, and no run
    /// resumes.
    pub(super) work: bool,
}

impl Waiting {
    /// The approval `notice` asks for, unless it asks none, is news that
    /// one was resolved, or comes from a group.
    fn from_notice(notice: &Notice) -> Option<Self> {
        let pending = notice.approval.as_ref()?;
        if notice.changed || notice.group {
            return None;
        }
        Some(Self {
            session: notice.session_id.clone(),
            approval: Asking {
                query_id: pending.query_id.clone(),
                app: pending.approval.app.clone(),
                action: pending.approval.action.clone(),
                fields: pending
                    .approval
                    .fields
                    .iter()
                    .cloned()
                    .map(Field::from)
                    .collect(),
            },
            work: pending.run_id.is_empty(),
        })
    }

    /// What live mode says when it asks.
    fn spoken(&self) -> String {
        let action = self.approval.action.trim().to_lowercase();
        format!("I need your OK to {action} in {}.", self.approval.app)
    }
}

impl Voice {
    /// The approvals waiting while live mode is on, oldest first.
    pub fn waiting(&self) -> Vec<Waiting> {
        self.state()
            .live
            .as_ref()
            .map(|s| s.asks.all())
            .unwrap_or_default()
    }

    /// Shows the approval `notice` asks for in the orb while live mode is
    /// on there, holding its notification back until live mode ends.
    /// Gives the notice back when live mode does not take it: off, in the
    /// bar, reading a voice note, or a notice that asks nothing.
    pub fn approval_notice(self: &Arc<Self>, notice: Notice) -> Option<Notice> {
        let Some(waiting) = Waiting::from_notice(&notice) else {
            return Some(notice);
        };
        if bar::is_open(&self.app) {
            return Some(notice);
        }
        let (id, new) = {
            let mut state = self.state();
            let Some(s) = state.live.as_mut().filter(|s| !s.note) else {
                return Some(notice);
            };
            (s.id, s.asks.add(waiting.clone(), Some(notice)))
        };
        if new {
            self.emit(LiveEvent::Approval(waiting.clone()));
            self.toast(NEEDS_OK);
            self.cue(id, &waiting);
        }
        None
    }

    /// Approval `query_id` no longer waits: answered as background work or
    /// elsewhere. The orb drops its card.
    pub fn withdrawn(&self, query_id: &str) {
        let removed = self
            .state()
            .live
            .as_mut()
            .and_then(|s| s.asks.remove(query_id));
        if removed.is_some() {
            self.emit(LiveEvent::Withdrawn {
                query_id: query_id.to_owned(),
            });
        }
    }

    /// Says that `waiting` needs an OK, when nothing else is said or heard
    /// right then: live mode listens, or waits on an approval, with no line
    /// playing. The words stay on the orb's card either way.
    fn cue(self: &Arc<Self>, id: u64, waiting: &Waiting) {
        let text = waiting.spoken();
        let mut state = self.state();
        let Some(s) = state.live.as_mut().filter(|s| s.id == id) else {
            return;
        };
        let quiet = matches!(s.live.phase(), Phase::Listening | Phase::Waiting);
        if !quiet || s.playing.is_some() {
            return;
        }
        let me = self.clone();
        let task = tauri::async_runtime::spawn(async move {
            if let Err(e) = me.say_once(id, text).await {
                log::debug!("saying an approval waits: {e:#}");
            }
        });
        s.tasks.push(task.inner().abort_handle());
    }

    /// Plays `text` once, outside the reader: nothing follows it.
    async fn say_once(&self, id: u64, text: String) -> anyhow::Result<()> {
        let note = self.beep.call(Say { text }).await?;
        let audio = self.audio(id).await.context("live mode ended")?;
        let path = self.line_path()?;
        tokio::fs::write(&path, &note.audio.data)
            .await
            .with_context(|| format!("writing {}", path.display()))?;
        let played = audio.play(&path).await;
        if let Err(e) = tokio::fs::remove_file(&path).await {
            log::debug!("removing {}: {e}", path.display());
        }
        let (_, done) = played?;
        // Stopped playback ends it as well.
        let _ = done.await;
        Ok(())
    }

    /// The reply in live session `id` asks before acting: the orb shows a
    /// card for it.
    pub(super) fn asking(&self, id: u64, approval: Asking) {
        let waiting = {
            let mut state = self.state();
            let Some(s) = state.live.as_mut().filter(|s| s.id == id) else {
                return;
            };
            let Some(chat) = s.chat else {
                return log::warn!("an approval came for a reply without a chat");
            };
            let waiting = Waiting {
                session: chat.to_string(),
                approval,
                work: false,
            };
            if !s.asks.add(waiting.clone(), None) {
                return;
            }
            waiting
        };
        self.emit(LiveEvent::Approval(waiting));
    }

    /// An approval was answered: the bar and the orb both drop its card,
    /// and the one that did not answer can follow the run it resumed. When
    /// live mode waits on that approval, the rest of the reply is read out
    /// from what is told to the `Teller` returned.
    pub fn answered(self: &Arc<Self>, answered: Answered) -> Option<Teller> {
        let Answered {
            session,
            query_id,
            run,
            decision,
        } = answered;
        let (teller, heard) = news::channel();
        let follow = {
            let mut state = self.state();
            if let Some(s) = state.live.as_mut()
                && let Some(asked) = s.asks.remove(&query_id)
            {
                // Only the live reply's own approval resumes what it reads.
                let ours = !asked.work && s.chat.is_some_and(|c| c.to_string() == session);
                match s.live.phase() {
                    Phase::Waiting if ours => {
                        s.run = Some(run.clone());
                        s.run_done = false;
                        Some((s.id, Some(heard)))
                    }
                    // The approval came while what was before it is
                    // still read out: the rest follows that.
                    Phase::Thinking | Phase::Speaking if ours => {
                        s.resume = Some(heard);
                        Some((s.id, None))
                    }
                    _ => None,
                }
            } else {
                None
            }
        };
        self.emit(LiveEvent::Answered {
            session,
            query_id,
            run,
            decision,
        });
        let (id, heard) = follow?;
        if let Some(heard) = heard {
            self.input(id, Input::Answered);
            self.follow_reply(id, heard, false);
        }
        Some(teller)
    }

    /// The live reply stopped on an approval: says so once it is quiet.
    pub(super) fn cue_reply(self: &Arc<Self>, id: u64) {
        let waiting = {
            let state = self.state();
            let Some(s) = state.live.as_ref().filter(|s| s.id == id) else {
                return;
            };
            let chat = s.chat.map(|c| c.to_string());
            s.asks
                .all()
                .into_iter()
                .find(|w| !w.work && Some(&w.session) == chat.as_ref())
        };
        if let Some(waiting) = waiting {
            self.cue(id, &waiting);
        }
    }
}
