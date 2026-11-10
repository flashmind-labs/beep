//! Keeps a followed session's newest page and cursor on disk, joined from the
//! updates as the webview joins them: when its run ends or it idles, when its
//! window hides, when the follow ends, and at most once a minute while it runs.

use super::{
    UpdateKind,
    pace::{Pushed, running},
};
use crate::agents::{
    kept_transcripts::{KeptTranscript, KeptTranscripts, SessionKey},
    sessions::SessionPage,
};
use beep_proto::node::agent::SessionCursor;
use serde_json::Value;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

/// How often, at most, a running session's page is written.
const RUNNING_EVERY: Duration = Duration::from_secs(60);

/// A page with its updates joined, and where it ends.
pub(super) struct Held {
    pub(super) page: Value,
    pub(super) cursor: SessionCursor,
}

pub(super) struct Keeper {
    kept: Arc<KeptTranscripts>,
    key: SessionKey,
    held: Option<Held>,
    /// The held page changed since it was written.
    dirty: bool,
    written: Option<Instant>,
    /// The page kept before this follow was read as the base for its first update.
    loaded: bool,
}

impl Keeper {
    pub(super) fn new(kept: Arc<KeptTranscripts>, key: SessionKey) -> Self {
        Self {
            kept,
            key,
            held: None,
            dirty: false,
            written: None,
            loaded: false,
        }
    }

    /// Joins `pushed` to the held page and writes it if due at `now`.
    pub(super) async fn take(&mut self, pushed: &Pushed, now: Instant) {
        let append = matches!(pushed.kind, UpdateKind::Delta) || pushed.resumed;

        // A follow going on from the webview's cursor joins the page kept before.
        if append && self.held.is_none() && !self.loaded {
            self.loaded = true;
            self.held = match self.kept.read(self.key.clone()).await {
                Ok(Some(KeptTranscript {
                    page,
                    cursor: Some(cursor),
                })) => Some(Held { page, cursor }),
                Ok(_) => None,
                Err(e) => {
                    log::warn!("reading the kept transcript: {e}");
                    None
                }
            };
        }

        let page = match serde_json::to_value(SessionPage::from(pushed.view.clone())) {
            Ok(page) => page,
            Err(e) => {
                log::warn!("keeping the transcript: {e}");
                return;
            }
        };
        self.held = join(self.held.take(), append, page, &pushed.cursor);
        self.dirty = self.held.is_some();

        if !running(&pushed.view)
            || self
                .written
                .is_none_or(|written| now.saturating_duration_since(written) >= RUNNING_EVERY)
        {
            self.write(now);
        }
    }

    /// When the held page is next due while the session runs.
    pub(super) fn due(&self) -> Option<Instant> {
        if !self.dirty {
            return None;
        }

        self.written.map(|written| written + RUNNING_EVERY)
    }

    /// Writes the held page at `now` if it changed.
    pub(super) fn write(&mut self, now: Instant) {
        let Some(held) = self.held.as_ref().filter(|_| self.dirty) else {
            return;
        };
        self.kept.keep(
            self.key.clone(),
            KeptTranscript {
                page: held.page.clone(),
                cursor: Some(held.cursor.clone()),
            },
        );
        self.dirty = false;
        self.written = Some(now);
    }
}

impl Drop for Keeper {
    fn drop(&mut self) {
        self.write(Instant::now());
    }
}

/// `page`, ending at `cursor`, joined after `held` when it `append`s, as the
/// webview's transcript cache joins it, then cut to start at `cursor.start`;
/// `None` when it does not continue `held` or does not fit its cursor.
pub(super) fn join(
    held: Option<Held>,
    append: bool,
    mut page: Value,
    cursor: &SessionCursor,
) -> Option<Held> {
    let at = page.get("earlier")?.as_u64()?;
    let Value::Array(fresh) = page.get_mut("entries")?.take() else {
        return None;
    };

    if at.checked_add(fresh.len() as u64)? != cursor.next || cursor.start > cursor.next {
        return None;
    }

    let (mut entries, earlier) = if append {
        let mut held = held?.page;
        let from = held.get("earlier")?.as_u64()?;
        let Value::Array(mut entries) = held.get_mut("entries")?.take() else {
            return None;
        };
        let keep = usize::try_from(at.checked_sub(from)?).ok()?;

        if keep > entries.len() {
            return None;
        }

        entries.truncate(keep);
        entries.extend(fresh);
        (entries, from)
    } else {
        (fresh, at)
    };

    let cut = usize::try_from(cursor.start.checked_sub(earlier)?).ok()?;

    if cut > entries.len() {
        return None;
    }

    entries.drain(..cut);
    page["entries"] = Value::Array(entries);
    page["earlier"] = cursor.start.into();
    Some(Held {
        page,
        cursor: cursor.clone(),
    })
}

#[cfg(test)]
#[path = "../../tests/agents_follow_keep.rs"]
mod tests;
