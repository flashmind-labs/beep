//! A session's transcript as the apps and the search index read it.

use super::{
    Session, child_reach, family::Family, limits::MAX_TRANSCRIPT, listings::Listings,
    sessions::Sessions,
};
use anyhow::Result;
use beep_proto::SessionId;
use beep_proto::node::agent::{self, Entry};
use beep_proto::watched;
use std::sync::Arc;

/// A session's transcript as the apps and the search index read it: the
/// live entries with their child cards while it runs here, else the file
/// an agent left.
pub(crate) struct Transcripts {
    sessions: Arc<Sessions>,
    listings: Arc<Listings>,
    family: Arc<Family>,
}

impl Transcripts {
    pub(crate) fn new(
        sessions: Arc<Sessions>,
        listings: Arc<Listings>,
        family: Arc<Family>,
    ) -> Self {
        Self {
            sessions,
            listings,
            family,
        }
    }

    /// The last `last` entries of a session this node runs and how many
    /// came before them, each Codex subagent among them with the steps
    /// its rollout holds: codex-acp sends none of them.
    pub(super) async fn live_transcript(
        &self,
        session: &Session,
        last: usize,
        before: Option<u64>,
    ) -> (Vec<agent::Entry>, u64) {
        let history = self.listings.claude_history(session, last, before).await;
        let (native, dropped) = history.as_ref().map_or((0, 0), |h| (h.native, h.dropped));
        let live_before = before.map(|end| end.saturating_sub(native).saturating_add(dropped));
        let (mut entries, earlier) = self
            .listings
            .codex_transcript(session, last, live_before)
            .await;

        if session.agent == "claude" {
            let (id, calls) = {
                watched::read(&session.live, |live| {
                    (
                        live.agent_session.clone(),
                        live.claude_children(earlier, entries.len()),
                    )
                })
            };

            if let Some(id) = id
                && !calls.is_empty()
            {
                match self
                    .listings
                    .external()
                    .attach_children(&id, calls, entries.clone())
                    .await
                {
                    Ok(linked) => entries = linked,
                    Err(e) => log::debug!("native child conversations not linked: {e}"),
                }
            }
        }

        child_reach::show_children(&self.family, session, &mut entries);
        let start = native + earlier.saturating_sub(dropped);
        if let Some(history) = history
            && earlier == dropped
            && entries.len() < last
        {
            let end = before.unwrap_or(native).min(native);
            if end > 0 {
                match self
                    .listings
                    .external()
                    .page(history.path, last - entries.len(), Some(end))
                    .await
                {
                    Ok(mut page) => {
                        page.entries.extend(entries);
                        return (page.entries, page.earlier);
                    }
                    Err(error) => log::warn!("native history page not recovered: {error:#}"),
                }
            } else if entries.is_empty() {
                return (entries, 0);
            }
        }

        (entries, start)
    }

    pub(crate) async fn transcript_window(
        &self,
        session: &SessionId,
        last: usize,
        before: Option<u64>,
        followed: Option<(&crate::logs::claude::Followed, Option<u64>)>,
        keep_pictures: bool,
    ) -> Result<Option<(Vec<Entry>, u64)>> {
        if let Some(found) = self.sessions.get(session) {
            return Ok(Some(self.live_transcript(&found, last, before).await));
        }

        self.listings
            .found_transcript(session, last, before, followed, keep_pictures)
            .await
    }

    /// The last `last` entries of a session before entry `before`, with how
    /// many come earlier: `None` for a session this node does not have.
    /// Search indexing reads without keeping the prompts' pictures.
    pub(crate) async fn read_transcript(
        &self,
        session: &SessionId,
        last: u32,
        before: Option<u64>,
        keep_pictures: bool,
    ) -> Result<Option<(Vec<agent::Entry>, u64)>> {
        let last = usize::try_from(last.min(MAX_TRANSCRIPT)).unwrap_or(usize::MAX);
        self.transcript_window(session, last, before, None, keep_pictures)
            .await
    }
}
