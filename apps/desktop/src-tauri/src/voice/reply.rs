//! Sending what was said and following the reply: how its stream ended,
//! and repeating it.

use super::{
    Voice,
    event::LiveEvent,
    news::{self, Heard},
    reader::{Feed, Voiced},
};
use crate::beep::{Pick, RunNews, Sent};
use beep_live::{Input, Reading};
use std::sync::Arc;
use tauri::ipc::Channel;
use uuid::Uuid;

const NOT_SENT: &str = "Could not send";
const NO_ANSWER: &str = "Beep could not answer";

/// How a reply's stream ended.
#[derive(Debug, Default)]
pub(super) struct Outcome {
    pub(super) started: bool,
    pub(super) failed: Option<String>,
    pub(super) status: Option<String>,
    pub(super) asked: bool,
}

impl Outcome {
    /// Whether the run stopped on an approval.
    fn waits(&self) -> bool {
        match self.status.as_deref() {
            Some(status) => status == "waiting",
            None => self.asked && self.failed.is_none(),
        }
    }

    fn ok(&self) -> bool {
        self.failed.is_none()
            && (self.waits() || matches!(self.status.as_deref(), Some("completed" | "cancelled")))
    }
}

/// The sentences of `text`, as the reader says them.
fn sentences(text: &str) -> Vec<String> {
    let mut reading = Reading::default();
    let mut lines = reading.push(text);
    lines.extend(reading.finish());
    lines
}

impl Voice {
    /// Sends what was said, and reads the reply out as it streams.
    pub(super) fn send(self: &Arc<Self>, id: u64, text: String, new_chat: bool) {
        let (chat, reply, dropped) = {
            let mut state = self.state();
            let Some(s) = state.live.as_mut().filter(|s| s.id == id) else {
                return;
            };
            let chat = if new_chat {
                Uuid::new_v4()
            } else {
                s.chat.unwrap_or_else(Uuid::new_v4)
            };
            s.chat_before = s.chat;
            s.chat = Some(chat);
            s.title = None;
            s.reply += 1;
            s.run = None;
            s.run_done = false;
            s.reply_text.clear();
            s.resume = None;
            text.clone_into(&mut s.said);
            s.last_reply.clear();
            s.prev = None;
            s.line = None;
            s.step = None;
            // A new message leaves the question unanswered on the server.
            let dropped = s
                .chat_before
                .map(|c| s.asks.drop_turns_of(&c.to_string()))
                .unwrap_or_default();
            (chat, s.reply, dropped)
        };
        for query_id in dropped {
            self.emit(LiveEvent::Withdrawn { query_id });
        }
        let (teller, heard) = news::channel();
        let beep = self.beep.clone();
        let me = self.clone();
        // The run goes on in the chat whatever happens to the reader, so
        // it marks itself over here.
        tauri::async_runtime::spawn(async move {
            let sent = Sent {
                message: text,
                images: Vec::new(),
                files: Vec::new(),
                research: false,
                voice: None,
                spoken: true,
            };
            // The chat shows the run when it is opened; nothing here draws it.
            let quiet = Channel::new(|_| Ok(()));
            let answer = beep
                .post_heard(chat, sent, Pick::default(), quiet, Some(&teller))
                .await;
            if let Err(e) = &answer {
                log::warn!("sending a spoken message: {e}");
                teller.tell(RunNews::Failed(e.to_string()));
            }
            me.run_over(id, reply);
            // Dropping the teller ends the reader's stream.
        });
        self.follow_reply(id, heard, true);
    }

    /// The run answering message `reply` of session `id` is over.
    fn run_over(&self, id: u64, reply: u64) {
        if let Some(s) = self
            .state()
            .live
            .as_mut()
            .filter(|s| s.id == id && s.reply == reply)
        {
            s.run = None;
            s.run_done = true;
        }
    }

    /// Reads the reply streaming into `heard` aloud, then tells live mode
    /// how it ended. `announce` says it answers a message just sent, which
    /// the chat shows once its run started.
    pub(super) fn follow_reply(self: &Arc<Self>, id: u64, heard: Heard, announce: bool) {
        let me = self.clone();
        let reader = async move {
            let said = me
                .state()
                .live
                .as_ref()
                .filter(|s| s.id == id)
                .map(|s| s.said.clone())
                .unwrap_or_default();
            let voiced = Voiced::new(0);
            let feed = Feed::Reply {
                heard,
                said,
                announce,
            };
            let outcome = me.read(id, feed, &voiced).await;
            me.reply_ended(id, outcome);
        };
        if let Some(s) = self.state().live.as_mut().filter(|s| s.id == id) {
            s.reader.start(reader);
        }
    }

    /// The reply's stream ended: back to listening, waiting on an
    /// approval, or saying it failed.
    fn reply_ended(self: &Arc<Self>, id: u64, outcome: Outcome) {
        log::debug!("live {id}: the reply ended, {outcome:?}");
        let resume = {
            let mut state = self.state();
            let Some(s) = state.live.as_mut().filter(|s| s.id == id) else {
                return;
            };
            if !outcome.waits() {
                s.run = None;
                s.run_done = true;
            }
            if !outcome.started && !outcome.ok() {
                // Nothing reached the server: no chat to stay on.
                s.chat = s.chat_before;
            }
            if outcome.waits() {
                s.resume.take()
            } else {
                None
            }
        };
        let input = if outcome.ok() {
            if let Some(heard) = resume {
                // Answered already: the rest of the reply goes on.
                return self.follow_reply(id, heard, false);
            }
            if outcome.waits() {
                Input::Approval
            } else {
                Input::SpeakDone
            }
        } else if outcome.started {
            if let Some(why) = &outcome.failed {
                log::warn!("a spoken reply failed: {why}");
            }
            self.toast(NO_ANSWER);
            Input::ReplyFailed
        } else {
            self.toast(NOT_SENT);
            Input::NotSent
        };
        let asked = matches!(input, Input::Approval);
        self.input(id, input);
        if asked {
            self.cue_reply(id);
        }
    }

    /// Reads the last reply again. False when there is nothing to read.
    pub(super) fn repeat(self: &Arc<Self>, id: u64) -> bool {
        let mut state = self.state();
        let Some(s) = state.live.as_mut().filter(|s| s.id == id) else {
            return true;
        };
        s.hush();
        let mut lines = sentences(&s.reply_text);
        if lines.is_empty() {
            lines = s.last_reply.clone();
        }
        if lines.is_empty() {
            return false;
        }
        let me = self.clone();
        s.reader.start(async move {
            let voiced = Voiced::new(0);
            me.read(id, Feed::Lines(lines), &voiced).await;
            me.input(id, Input::SpeakDone);
        });
        true
    }

    /// The run answering session `id`'s message started. A message just
    /// sent, `said`, now shows in its chat.
    pub(super) fn started(&self, id: u64, run: String, said: Option<&str>) {
        let chat = {
            let mut state = self.state();
            let Some(s) = state.live.as_mut().filter(|s| s.id == id) else {
                return;
            };
            s.run = Some(run.clone());
            s.chat
        };
        let (Some(chat), Some(said)) = (chat, said) else {
            return;
        };
        self.beep.add_live_chat(chat);
        self.beep.set_session(Some(chat.to_string()));
        self.emit(LiveEvent::Sent {
            session: chat.to_string(),
            text: said.to_owned(),
            run,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reply_that_failed_or_never_started_is_not_ok() {
        let done = |status: &str| Outcome {
            started: true,
            status: Some(status.into()),
            ..Outcome::default()
        };
        assert!(done("completed").ok());
        assert!(done("cancelled").ok());
        assert!(done("waiting").ok() && done("waiting").waits());
        assert!(!done("timed_out").ok());
        let errored = Outcome {
            failed: Some("internal error".into()),
            ..done("completed")
        };
        assert!(!errored.ok());
        // A stream that ended with neither a status nor an error.
        assert!(!Outcome::default().ok());
    }

    #[test]
    fn a_reply_is_repeated_sentence_by_sentence() {
        assert_eq!(
            sentences("Two emails need a reply. Alex asks about Thursday."),
            vec!["Two emails need a reply.", "Alex asks about Thursday."]
        );
        assert!(sentences("  ").is_empty());
    }
}
