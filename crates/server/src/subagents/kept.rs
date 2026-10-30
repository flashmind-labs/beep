//! Sub-agents kept after they report, so `background message` can tell them more
//! with their history intact. Each chat keeps a few, and one left idle for
//! an hour goes.

use super::work::WorkId;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::{Duration, Instant},
};
use uuid::Uuid;

/// Sub-agents one chat keeps. Past it the one idle longest goes.
pub(crate) const MAX_PER_CHAT: usize = 5;
/// How long an idle sub-agent is kept after its last report.
pub(crate) const KEEP_IDLE: Duration = Duration::from_secs(60 * 60);

/// One kept sub-agent and who it belongs to.
pub(crate) struct Kept<S> {
    pub(crate) session: S,
    user: String,
    chat: Uuid,
    /// Held while deciding whether it runs, so a new message and the end of
    /// its work never both think it idle.
    status: Mutex<Status>,
}

/// Whether a kept sub-agent runs, and what waits for it.
#[derive(Debug, Default)]
pub(crate) struct Status {
    /// When it last went idle; `None` while it runs.
    pub(crate) idle_since: Option<Instant>,
    /// Messages sent while it ran, for when its current run ends. Kept here
    /// rather than queued in the agent, so a cancel can drop them.
    pub(crate) queued: Vec<String>,
}

/// What a run of a kept sub-agent does once it ends.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Next {
    /// Go on with the messages sent meanwhile.
    Continue(String),
    /// Report, and stay idle from `now`.
    Idle,
}

impl Status {
    /// Decides what follows a run that just ended. A cancelled run drops
    /// the messages queued for it, so nothing runs that nobody follows.
    pub(crate) fn after_run(&mut self, cancelled: bool, now: Instant) -> Next {
        let queued = std::mem::take(&mut self.queued);
        if cancelled || queued.is_empty() {
            self.idle_since = Some(now);
            return Next::Idle;
        }
        Next::Continue(queued.join("\n\n"))
    }
}

impl<S> Kept<S> {
    /// The lock that orders a new message against the end of its work.
    pub(crate) fn lock(&self) -> MutexGuard<'_, Status> {
        self.status.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn idle_since(&self) -> Option<Instant> {
        self.lock().idle_since
    }
}

/// Every kept sub-agent on the server, by work id.
pub(crate) struct KeptAgents<S>(Mutex<HashMap<WorkId, Arc<Kept<S>>>>);

impl<S> Default for KeptAgents<S> {
    fn default() -> Self {
        Self(Mutex::default())
    }
}

impl<S> KeptAgents<S> {
    fn map(&self) -> MutexGuard<'_, HashMap<WorkId, Arc<Kept<S>>>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Keeps `session`, running now, as `id` of `user` in `chat`, making
    /// room in the chat if it is full.
    pub(crate) fn keep(
        &self,
        id: WorkId,
        user: &str,
        chat: Uuid,
        session: S,
        now: Instant,
    ) -> Arc<Kept<S>> {
        let mut map = self.map();
        expire(&mut map, now);
        let mut idle: Vec<(Instant, WorkId)> = map
            .iter()
            .filter(|(_, kept)| kept.chat == chat)
            .filter_map(|(id, kept)| kept.idle_since().map(|at| (at, *id)))
            .collect();
        let in_chat = map.values().filter(|kept| kept.chat == chat).count();
        idle.sort();
        for (_, oldest) in idle
            .into_iter()
            .take((in_chat + 1).saturating_sub(MAX_PER_CHAT))
        {
            map.remove(&oldest);
        }
        let kept = Arc::new(Kept {
            session,
            user: user.to_owned(),
            chat,
            status: Mutex::default(),
        });
        map.insert(id, kept.clone());
        kept
    }

    /// The kept sub-agent `id`, when it is `user`'s in `chat`.
    pub(crate) fn get(
        &self,
        id: WorkId,
        user: &str,
        chat: Uuid,
        now: Instant,
    ) -> Option<Arc<Kept<S>>> {
        let mut map = self.map();
        expire(&mut map, now);
        map.get(&id)
            .filter(|kept| kept.user == user && kept.chat == chat)
            .cloned()
    }

    /// Whether `id` can still be messaged.
    #[cfg(test)]
    pub(crate) fn contains(&self, id: WorkId) -> bool {
        self.map().contains_key(&id)
    }
}

/// Drops the sub-agents idle for longer than [`KEEP_IDLE`].
fn expire<S>(map: &mut HashMap<WorkId, Arc<Kept<S>>>, now: Instant) {
    map.retain(|_, kept| {
        kept.idle_since()
            .is_none_or(|at| now.saturating_duration_since(at) < KEEP_IDLE)
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: &str) -> WorkId {
        n.parse().expect("a work id")
    }

    #[test]
    fn a_chat_keeps_a_few_and_drops_the_longest_idle() {
        let kept = KeptAgents::default();
        let chat = Uuid::new_v4();
        let start = Instant::now();
        for n in 1..=MAX_PER_CHAT {
            let agent = kept.keep(id(&n.to_string()), "alice", chat, n, start);
            // Each goes idle a second after the one before.
            agent.lock().idle_since = Some(start + Duration::from_secs(n as u64));
        }
        kept.keep(id("99"), "alice", chat, 99, start);
        assert!(!kept.contains(id("1")), "the longest idle goes");
        assert!(kept.contains(id("2")));
        assert!(kept.contains(id("99")));
        assert!(kept.get(id("2"), "bob", chat, start).is_none());
        assert!(kept.get(id("2"), "alice", Uuid::new_v4(), start).is_none());
    }

    #[test]
    fn an_idle_agent_expires_but_a_running_one_stays() {
        let kept = KeptAgents::default();
        let chat = Uuid::new_v4();
        let start = Instant::now();
        let idle = kept.keep(id("1"), "alice", chat, (), start);
        idle.lock().idle_since = Some(start);
        kept.keep(id("2"), "alice", chat, (), start);
        let later = start + KEEP_IDLE;
        assert!(kept.get(id("1"), "alice", chat, later).is_none());
        assert!(kept.get(id("2"), "alice", chat, later).is_some());
    }

    #[test]
    fn a_cancelled_run_drops_its_queued_messages() {
        let now = Instant::now();
        let mut status = Status {
            idle_since: None,
            queued: vec!["Also check spam.".to_owned(), "And archive.".to_owned()],
        };
        assert_eq!(
            status.after_run(false, now),
            Next::Continue("Also check spam.\n\nAnd archive.".to_owned())
        );
        assert_eq!(status.idle_since, None, "it goes on");

        status.queued.push("One more thing.".to_owned());
        assert_eq!(status.after_run(true, now), Next::Idle);
        assert!(status.queued.is_empty());
        assert_eq!(status.idle_since, Some(now));
        assert_eq!(
            status.after_run(false, now),
            Next::Idle,
            "nothing left to run"
        );
    }
}
