//! The replies being written now: at most one per chat and a few in all. Each has a token that
//! stops it, which the stop button, a closed window and a failing event channel all use. A reply
//! may also wait for the user to allow an MCP tool call: the question lives here until answered.

use crate::error::{DesktopError, Result};
use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard, PoisonError},
};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Replies running at once, over all chats.
const MAX_RUNS: usize = 4;

struct Running {
    cancel: CancellationToken,
    /// Fires when the reply is over, for an answer that waits for the end of it.
    over: CancellationToken,
    /// The window that asked, whose closing stops it.
    window: String,
}

/// A tool call waiting for the user, in the chat it belongs to.
struct Waiting {
    chat: Uuid,
    reply: oneshot::Sender<bool>,
}

#[derive(Default)]
pub(crate) struct Runs {
    running: Mutex<HashMap<Uuid, Running>>,
    /// The questions on screen, by id.
    asking: Mutex<HashMap<String, Waiting>>,
}

impl Runs {
    fn lock(&self) -> MutexGuard<'_, HashMap<Uuid, Running>> {
        self.running.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Registers a reply for chat `id`, or says why it cannot run now.
    pub(crate) fn begin(&self, id: Uuid, window: &str) -> Result<CancellationToken> {
        let mut running = self.lock();
        if running.contains_key(&id) {
            return Err(DesktopError::BadRequest(
                "This chat is already answering.".into(),
            ));
        }
        if running.len() >= MAX_RUNS {
            return Err(DesktopError::BadRequest(
                "Too many replies are running. Stop one first.".into(),
            ));
        }
        let cancel = CancellationToken::new();
        running.insert(
            id,
            Running {
                cancel: cancel.clone(),
                over: CancellationToken::new(),
                window: window.to_owned(),
            },
        );
        Ok(cancel)
    }

    pub(crate) fn finish(&self, id: Uuid) {
        let run = self.lock().remove(&id);
        if let Some(run) = run {
            run.over.cancel();
        }
        // A question of this chat is moot now: its receiver is dropped, which reads as a no.
        self.asking_lock().retain(|_, waiting| waiting.chat != id);
    }

    fn asking_lock(&self) -> MutexGuard<'_, HashMap<String, Waiting>> {
        self.asking.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Registers question `query_id` of chat `chat`; the receiver gets the answer.
    pub(crate) fn ask(&self, chat: Uuid, query_id: String) -> oneshot::Receiver<bool> {
        let (reply, answer) = oneshot::channel();
        self.asking_lock().insert(query_id, Waiting { chat, reply });
        answer
    }

    /// Gives the user's answer to a question of chat `chat`. False when it is not waiting.
    pub(crate) fn answer(&self, chat: Uuid, query_id: &str, allow: bool) -> bool {
        let mut asking = self.asking_lock();
        match asking.remove(query_id) {
            Some(waiting) if waiting.chat == chat => waiting.reply.send(allow).is_ok(),
            Some(waiting) => {
                // Not this chat's question: put it back.
                asking.insert(query_id.to_owned(), waiting);
                false
            }
            None => false,
        }
    }

    /// A token that fires when chat `id`'s reply ends; `None` when none is running.
    pub(crate) fn over(&self, id: Uuid) -> Option<CancellationToken> {
        self.lock().get(&id).map(|run| run.over.clone())
    }

    /// The chats answering now.
    pub(crate) fn ids(&self) -> Vec<Uuid> {
        self.lock().keys().copied().collect()
    }

    /// Stops chat `id`'s reply; what streamed so far is kept.
    pub(crate) fn stop(&self, id: Uuid) {
        if let Some(run) = self.lock().get(&id) {
            run.cancel.cancel();
        }
    }

    /// Stops the replies `window` asked for, as it closes.
    pub(crate) fn window_closed(&self, window: &str) {
        for run in self.lock().values().filter(|run| run.window == window) {
            run.cancel.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_reply_per_chat_and_a_bounded_number_in_all() {
        let runs = Runs::default();
        let first = Uuid::new_v4();
        runs.begin(first, "center").unwrap();
        assert!(runs.begin(first, "center").is_err());
        for _ in 1..MAX_RUNS {
            runs.begin(Uuid::new_v4(), "center").unwrap();
        }
        assert!(runs.begin(Uuid::new_v4(), "center").is_err());
        runs.finish(first);
        assert!(runs.begin(Uuid::new_v4(), "center").is_ok());
    }

    #[test]
    fn stop_and_a_closed_window_cancel_the_token() {
        let runs = Runs::default();
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let (ta, tb) = (runs.begin(a, "one").unwrap(), runs.begin(b, "two").unwrap());
        runs.stop(a);
        assert!(ta.is_cancelled() && !tb.is_cancelled());
        runs.window_closed("two");
        assert!(tb.is_cancelled());
    }
}
