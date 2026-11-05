//! The replies being written now: at most one per chat and a few in all. Each has a token that
//! stops it, which the stop button, a closed window and a failing event channel all use.

use crate::error::{DesktopError, Result};
use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard, PoisonError},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Replies running at once, over all chats.
const MAX_RUNS: usize = 4;

struct Running {
    cancel: CancellationToken,
    /// The window that asked, whose closing stops it.
    window: String,
}

#[derive(Default)]
pub(crate) struct Runs {
    running: Mutex<HashMap<Uuid, Running>>,
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
                window: window.to_owned(),
            },
        );
        Ok(cancel)
    }

    pub(crate) fn finish(&self, id: Uuid) {
        self.lock().remove(&id);
    }

    pub(crate) fn is_running(&self, id: Uuid) -> bool {
        self.lock().contains_key(&id)
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
