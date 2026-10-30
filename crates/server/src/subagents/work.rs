//! The sub-agents and jobs the main agent started, per chat, and their
//! results on the way back to it.
//!
//! A result goes back once: to `background wait` when the agent waited for it, or
//! to the next turn of its chat. A turn claims every result waiting in its
//! chat as it is created (see [`Work::claim`]), so a message the user sends
//! carries them in its context. When the chat is idle a delivery starts a
//! turn of its own for them; it never starts one while another turn runs
//! there, and never one with nothing to hand back.

use super::Watch;
use chrono::{DateTime, Utc};
use std::{
    collections::{HashMap, HashSet},
    future::Future,
    sync::{Mutex, MutexGuard, PoisonError},
    time::Duration,
};
use tokio::{
    sync::{Notify, watch},
    time::{Instant, sleep_until, timeout},
};
use uuid::Uuid;

/// A sub-agent's or job's id, as the agent writes it: `w12`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkId(u64);

impl std::fmt::Display for WorkId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "w{}", self.0)
    }
}

impl std::str::FromStr for WorkId {
    type Err = UnknownWork;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let text = text.trim();
        let digits = text.strip_prefix('w').unwrap_or(text);
        digits.parse().map(WorkId).map_err(|_| UnknownWork)
    }
}

/// No sub-agent or job of this chat has that id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("no agent or job of this chat has that id; `background list` lists them")]
pub struct UnknownWork;

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr)]
#[strum(serialize_all = "lowercase")]
pub enum WorkKind {
    Agent,
    Job,
}

/// When a result goes back to its chat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Delivery {
    /// In a turn of its own as soon as the chat is free.
    #[default]
    Now,
    /// With the user's next message in the chat.
    Later,
}

/// How a sub-agent or job ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub ok: bool,
    /// Its report or result, already cut to size.
    pub text: String,
}

/// One line of `background list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    pub id: WorkId,
    pub kind: WorkKind,
    pub task: String,
    pub started: DateTime<Utc>,
    /// What it did last while running.
    pub step: String,
    pub outcome: Option<Outcome>,
}

/// A finished result to hand back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finished {
    pub id: WorkId,
    pub kind: WorkKind,
    pub task: String,
    pub outcome: Outcome,
}

/// Finished work is listed this long, then forgotten.
const KEPT: Duration = Duration::from_secs(60 * 60);
/// Most finished items one chat keeps listed.
const KEPT_PER_CHAT: usize = 20;
/// The longest `background wait` waits.
pub const MAX_WAIT: Duration = Duration::from_secs(120);

struct Item {
    kind: WorkKind,
    user: String,
    chat: Uuid,
    task: String,
    started: DateTime<Utc>,
    watch: Watch,
    delivery: Delivery,
    done: watch::Sender<Option<Outcome>>,
    finished_at: Option<Instant>,
    /// Its result went back, so it is never handed back again.
    handed_back: bool,
}

#[derive(Default)]
struct State {
    next: u64,
    items: HashMap<WorkId, Item>,
    /// Chats a delivery is running for.
    delivering: HashSet<Uuid>,
    /// Results a turn claimed, by chat, until its context takes them. A
    /// chat runs one turn at a time, so the chat names the turn.
    claimed: HashMap<Uuid, Vec<Finished>>,
}

impl State {
    fn prune(&mut self) {
        let now = Instant::now();
        self.items.retain(|_, item| {
            item.finished_at
                .is_none_or(|at| now.duration_since(at) < KEPT || !item.handed_back)
        });
        let mut chats: HashMap<Uuid, Vec<(Instant, WorkId)>> = HashMap::new();
        for (id, item) in &self.items {
            if let (Some(at), true) = (item.finished_at, item.handed_back) {
                chats.entry(item.chat).or_default().push((at, *id));
            }
        }
        for mut finished in chats.into_values() {
            if finished.len() > KEPT_PER_CHAT {
                finished.sort();
                let extra = finished.len() - KEPT_PER_CHAT;
                for (_, id) in finished.into_iter().take(extra) {
                    self.items.remove(&id);
                }
            }
        }
    }

    fn mine(&self, user: &str, chat: Uuid, id: WorkId) -> Result<&Item, UnknownWork> {
        self.items
            .get(&id)
            .filter(|item| item.user == user && item.chat == chat)
            .ok_or(UnknownWork)
    }

    /// Whether `chat` has results waiting that go back in a turn of their own.
    fn due(&self, chat: Uuid) -> bool {
        self.items.values().any(|item| {
            item.chat == chat
                && !item.handed_back
                && item.delivery == Delivery::Now
                && item.done.borrow().is_some()
        })
    }

    fn take_waiting(&mut self, chat: Uuid, delivery: Option<Delivery>) -> Vec<Finished> {
        let mut taken: Vec<Finished> = Vec::new();
        for (id, item) in &mut self.items {
            let ready = item.chat == chat
                && !item.handed_back
                && delivery.is_none_or(|delivery| item.delivery == delivery);
            if let (true, Some(outcome)) = (ready, item.done.borrow().clone()) {
                item.handed_back = true;
                taken.push(Finished {
                    id: *id,
                    kind: item.kind,
                    task: item.task.clone(),
                    outcome,
                });
            }
        }
        taken.sort_by_key(|finished| finished.id);
        taken
    }
}

/// What a finished item asks of the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AfterFinish {
    /// Start a delivery for the chat.
    Deliver,
    /// Nothing: it went back already, waits for the next message, or a
    /// delivery running for the chat takes it.
    Nothing,
}

/// Every chat's sub-agents and jobs, kept on [`super::Subagents`].
#[derive(Default)]
pub struct Work {
    state: Mutex<State>,
}

impl Work {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Lists a new running item and returns its id and the watch it runs
    /// under.
    pub fn start(
        &self,
        user: &str,
        chat: Uuid,
        kind: WorkKind,
        task: &str,
        delivery: Delivery,
    ) -> (WorkId, Watch) {
        let mut state = self.state();
        state.prune();
        state.next += 1;
        let id = WorkId(state.next);
        let watch = Watch::default();
        let (done, _) = watch::channel(None);
        state.items.insert(
            id,
            Item {
                kind,
                user: user.to_owned(),
                chat,
                task: task.to_owned(),
                started: Utc::now(),
                watch: watch.clone(),
                delivery,
                done,
                finished_at: None,
                handed_back: false,
            },
        );
        (id, watch)
    }

    /// Forgets an item that never ran, such as one refused for lack of room.
    pub fn forget(&self, id: WorkId) {
        self.state().items.remove(&id);
    }

    /// Records how `id` ended. [`AfterFinish::Deliver`] means the caller
    /// starts the chat's delivery, which [`Self::delivered`] ends.
    pub fn finish(&self, id: WorkId, outcome: Outcome) -> AfterFinish {
        let mut state = self.state();
        let Some(item) = state.items.get_mut(&id) else {
            return AfterFinish::Nothing;
        };
        item.finished_at = Some(Instant::now());
        item.done.send_replace(Some(outcome));
        let (chat, delivery, handed_back) = (item.chat, item.delivery, item.handed_back);
        if handed_back || delivery == Delivery::Later || !state.delivering.insert(chat) {
            return AfterFinish::Nothing;
        }
        AfterFinish::Deliver
    }

    /// Whether `chat` has results that start a turn of their own.
    pub fn due(&self, chat: Uuid) -> bool {
        self.state().due(chat)
    }

    /// Hands every result waiting in `chat` to the turn being created there,
    /// so nothing else hands them back; the turn's context takes them with
    /// [`Self::take_claimed`]. Returns how many it claimed.
    pub fn claim(&self, chat: Uuid) -> usize {
        let mut state = self.state();
        let taken = state.take_waiting(chat, None);
        let count = taken.len();
        if count > 0 {
            state.claimed.entry(chat).or_default().extend(taken);
        }
        count
    }

    /// The results the chat's running turn claimed, handed over once.
    pub fn take_claimed(&self, chat: Uuid) -> Vec<Finished> {
        self.state().claimed.remove(&chat).unwrap_or_default()
    }

    /// Ends the chat's delivery, unless results came in meanwhile; `true`
    /// then, and the delivery goes on with them.
    pub fn delivered(&self, chat: Uuid) -> bool {
        let mut state = self.state();
        let more = state.due(chat);
        if !more {
            state.delivering.remove(&chat);
        }
        more
    }

    /// Whether `id`, of `user` in `chat`, is still running.
    #[cfg(test)]
    pub fn is_running(&self, user: &str, chat: Uuid, id: WorkId) -> Result<bool, UnknownWork> {
        Ok(self.state().mine(user, chat, id)?.done.borrow().is_none())
    }

    /// Lists `id` as running again, for a sub-agent given a new message
    /// after it ended. Returns the watch it runs under and its last result
    /// when that never went back.
    pub fn resume(
        &self,
        user: &str,
        chat: Uuid,
        id: WorkId,
    ) -> Result<(Watch, Option<Outcome>), UnknownWork> {
        let mut state = self.state();
        state.mine(user, chat, id)?;
        let item = state.items.get_mut(&id).ok_or(UnknownWork)?;
        let unseen = item.done.borrow().clone().filter(|_| !item.handed_back);
        let watch = Watch::default();
        item.watch = watch.clone();
        item.done.send_replace(None);
        item.finished_at = None;
        item.handed_back = false;
        item.delivery = Delivery::Now;
        Ok((watch, unseen))
    }

    /// The chat's sub-agents and jobs, running first, then newest first.
    pub fn list(&self, user: &str, chat: Uuid) -> Vec<Listing> {
        let mut state = self.state();
        state.prune();
        let mut listed: Vec<Listing> = state
            .items
            .iter()
            .filter(|(_, item)| item.user == user && item.chat == chat)
            .map(|(id, item)| Listing {
                id: *id,
                kind: item.kind,
                task: item.task.clone(),
                started: item.started,
                step: item.watch.last_step(),
                outcome: item.done.borrow().clone(),
            })
            .collect();
        listed.sort_by_key(|listing| (listing.outcome.is_some(), std::cmp::Reverse(listing.id.0)));
        listed
    }

    /// Waits up to `wait` for `id` to end. Its result, when it ended, is
    /// handed back here and not delivered again.
    pub async fn wait(
        &self,
        user: &str,
        chat: Uuid,
        id: WorkId,
        wait: Duration,
    ) -> Result<Option<Outcome>, UnknownWork> {
        let mut done = self.state().mine(user, chat, id)?.done.subscribe();
        let ended = timeout(wait.min(MAX_WAIT), done.wait_for(Option::is_some)).await;
        if !matches!(ended, Ok(Ok(_))) {
            return Ok(None);
        }
        let mut state = self.state();
        let Some(item) = state.items.get_mut(&id) else {
            return Ok(None);
        };
        // A delivery may have taken it in the meantime; then this is a
        // second look at what already went back, which does no harm.
        item.handed_back = true;
        Ok(item.done.borrow().clone())
    }

    /// Stops `id` if it still runs, and nothing comes back from it;
    /// `false` when it had already ended.
    pub fn cancel(&self, user: &str, chat: Uuid, id: WorkId) -> Result<bool, UnknownWork> {
        let mut state = self.state();
        state.mine(user, chat, id)?;
        let Some(item) = state.items.get_mut(&id) else {
            return Err(UnknownWork);
        };
        if item.done.borrow().is_some() {
            return Ok(false);
        }
        item.handed_back = true;
        item.watch.cancel.cancel();
        Ok(true)
    }

    /// Gives up delivering to `chat`; its results wait for the next message.
    pub fn abandon_delivery(&self, chat: Uuid) {
        self.state().delivering.remove(&chat);
    }

    /// Running items of `user` in all chats.
    #[cfg(test)]
    pub fn running(&self, user: &str) -> usize {
        self.state()
            .items
            .values()
            .filter(|item| item.user == user && item.done.borrow().is_none())
            .count()
    }
}

/// What starting a delivery turn came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Submitted {
    /// A turn started and claimed the results.
    Started,
    /// A turn runs in the chat; try again when one ends.
    Busy,
    /// Nothing was left to hand back: a turn of the user's claimed it.
    Nothing,
}

/// The longest a delivery waits for its chat to be free.
pub const DELIVERY_PATIENCE: Duration = Duration::from_secs(2 * 60 * 60);
/// How often a waiting delivery tries again even if no turn end woke it.
const RETRY: Duration = Duration::from_secs(5);

/// Hands the chat's finished results back in turns of their own, one turn
/// at a time and never while another runs there. `submit` starts a turn
/// only when results are due and the chat is free, and the turn claims
/// them as it is created; `turn_ended` wakes the wait for a free chat.
/// Results stay waiting until a turn claims them, so a message the user
/// sends meanwhile takes them instead.
pub async fn deliver<S, F>(work: &Work, chat: Uuid, turn_ended: &Notify, submit: S)
where
    S: Fn() -> F,
    F: Future<Output = anyhow::Result<Submitted>>,
{
    let deadline = Instant::now() + DELIVERY_PATIENCE;
    loop {
        // Registered before trying, so a turn ending in between still wakes it.
        let ended = turn_ended.notified();
        tokio::pin!(ended);
        ended.as_mut().enable();
        match submit().await {
            Ok(Submitted::Started | Submitted::Nothing) => {
                if !work.delivered(chat) {
                    return;
                }
            }
            Ok(Submitted::Busy) if Instant::now() < deadline => {
                tokio::select! {
                    _ = ended => {}
                    _ = sleep_until(Instant::now() + RETRY) => {}
                }
            }
            Ok(Submitted::Busy) => {
                log::warn!(
                    "subagents: the chat stayed busy; results wait for the next message; chat={chat}"
                );
                work.abandon_delivery(chat);
                return;
            }
            Err(error) => {
                log::warn!("subagents: unable to deliver: {error:#}; chat={chat}");
                work.abandon_delivery(chat);
                return;
            }
        }
    }
}

/// The message of a turn that hands results back; the results themselves
/// are in its context.
pub const REPORT: &str = "Background work you started in this chat ended; its results are \
    above. Tell the user what came of it in a few sentences. Do not start it again unless \
    they ask.";

/// The context line for results a turn claimed, before the user's text.
pub fn claimed_context(finished: &[Finished]) -> String {
    format!(
        "Background work you started in this chat ended. Relay what came of it briefly, \
         leading with the result; when the user said something, answer that too.{}",
        results(finished)
    )
}

/// The results, one block each, for a turn or for the next message's context.
pub fn results(finished: &[Finished]) -> String {
    let mut text = String::new();
    for done in finished {
        let kind: &'static str = done.kind.into();
        let state = if done.outcome.ok {
            "finished"
        } else {
            "failed"
        };
        text.push_str(&format!(
            "\n\n{kind} {} ({}) {state}:\n{}",
            done.id, done.task, done.outcome.text
        ));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    fn ok(text: &str) -> Outcome {
        Outcome {
            ok: true,
            text: text.to_owned(),
        }
    }

    #[test]
    fn ids_read_with_or_without_their_letter() {
        assert_eq!("w7".parse(), Ok(WorkId(7)));
        assert_eq!(" 7 ".parse(), Ok(WorkId(7)));
        assert_eq!("job".parse::<WorkId>(), Err(UnknownWork));
        assert_eq!(WorkId(7).to_string(), "w7");
    }

    /// A chat the way `App` runs one: a turn at a time, each claiming the
    /// waiting results under the lock that creates it.
    struct Chat {
        work: Arc<Work>,
        id: Uuid,
        busy: Mutex<bool>,
        /// What each turn carried: the results its context took.
        turns: Mutex<Vec<Vec<Finished>>>,
    }

    impl Chat {
        fn new(work: &Arc<Work>) -> Arc<Self> {
            Arc::new(Self {
                work: work.clone(),
                id: Uuid::new_v4(),
                busy: Mutex::new(false),
                turns: Mutex::new(Vec::new()),
            })
        }

        /// `App::submit_locked`: refused while a turn runs, else it starts
        /// one that claims what waits.
        fn start(&self) -> bool {
            let mut busy = self.busy.lock().expect("not poisoned");
            if *busy {
                return false;
            }
            *busy = true;
            self.work.claim(self.id);
            let claimed = self.work.take_claimed(self.id);
            self.turns.lock().expect("not poisoned").push(claimed);
            true
        }

        /// `App::submit_delivery`: a turn only when results are due.
        fn deliver(&self) -> Submitted {
            let busy = self.busy.lock().expect("not poisoned");
            if !self.work.due(self.id) {
                return Submitted::Nothing;
            }
            drop(busy);
            if self.start() {
                Submitted::Started
            } else {
                Submitted::Busy
            }
        }

        fn end_turn(&self) {
            *self.busy.lock().expect("not poisoned") = false;
        }

        fn turns(&self) -> Vec<Vec<WorkId>> {
            self.turns
                .lock()
                .expect("not poisoned")
                .iter()
                .map(|turn| turn.iter().map(|finished| finished.id).collect())
                .collect()
        }
    }

    #[tokio::test]
    async fn a_result_delivers_in_a_turn_of_its_own_when_the_chat_is_idle() {
        let work = Arc::new(Work::default());
        let chat = Chat::new(&work);
        let (id, _) = work.start(
            "alice",
            chat.id,
            WorkKind::Job,
            "search mail",
            Delivery::Now,
        );
        assert_eq!(work.finish(id, ok("3 results")), AfterFinish::Deliver);
        deliver(&work, chat.id, &Notify::new(), || async {
            Ok(chat.deliver())
        })
        .await;
        assert_eq!(chat.turns(), [vec![id]]);
        let carried = &chat.turns.lock().expect("not poisoned")[0];
        assert!(claimed_context(carried).contains("job w1 (search mail) finished:\n3 results"));
        assert!(!work.due(chat.id), "it goes back once");
    }

    #[tokio::test]
    async fn a_result_waiting_when_the_user_speaks_joins_that_turn_and_wakes_nothing() {
        let work = Arc::new(Work::default());
        let chat = Chat::new(&work);
        let (id, _) = work.start("alice", chat.id, WorkKind::Agent, "dig", Delivery::Now);
        assert_eq!(work.finish(id, ok("found it")), AfterFinish::Deliver);
        // The user's message is created before the delivery gets to run.
        assert!(chat.start());
        chat.end_turn();
        deliver(&work, chat.id, &Notify::new(), || async {
            Ok(chat.deliver())
        })
        .await;
        assert_eq!(chat.turns(), [vec![id]], "no second turn");
        // The delivery ended, so the next result starts one again.
        let (next, _) = work.start("alice", chat.id, WorkKind::Job, "more", Delivery::Now);
        assert_eq!(work.finish(next, ok("x")), AfterFinish::Deliver);
    }

    #[tokio::test]
    async fn a_result_during_a_turn_waits_for_it_to_end_and_never_overlaps() {
        let work = Arc::new(Work::default());
        let chat = Chat::new(&work);
        let turn_ended = Arc::new(Notify::new());
        let tries = Arc::new(AtomicUsize::new(0));
        let (first, _) = work.start("alice", chat.id, WorkKind::Agent, "first", Delivery::Now);
        let (second, _) = work.start("alice", chat.id, WorkKind::Job, "second", Delivery::Now);
        // A turn of the user's runs, started before anything ended.
        assert!(chat.start());
        assert_eq!(work.finish(first, ok("one")), AfterFinish::Deliver);

        let delivering = {
            let (work, chat, turn_ended, tries) = (
                work.clone(),
                chat.clone(),
                turn_ended.clone(),
                tries.clone(),
            );
            tokio::spawn(async move {
                deliver(&work, chat.id, &turn_ended, || {
                    tries.fetch_add(1, Ordering::SeqCst);
                    let submitted = chat.deliver();
                    async move { Ok(submitted) }
                })
                .await;
            })
        };
        while tries.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
        // The second ends while the first waits; the same delivery takes it.
        assert_eq!(work.finish(second, ok("two")), AfterFinish::Nothing);
        chat.end_turn();
        turn_ended.notify_waiters();
        delivering.await.expect("the delivery does not panic");
        assert_eq!(chat.turns(), [vec![], vec![first, second]]);
    }

    #[tokio::test]
    async fn a_waited_result_is_not_delivered_again() {
        let work = Arc::new(Work::default());
        let chat = Uuid::new_v4();
        let (id, _) = work.start("alice", chat, WorkKind::Job, "slow", Delivery::Now);
        let waiting = {
            let work = work.clone();
            tokio::spawn(async move { work.wait("alice", chat, id, Duration::from_secs(5)).await })
        };
        tokio::task::yield_now().await;
        work.finish(id, ok("done"));
        let waited = waiting.await.expect("no panic");
        assert_eq!(waited, Ok(Some(ok("done"))));
        assert!(!work.due(chat));
        assert_eq!(work.claim(chat), 0);
        assert!(!work.delivered(chat));
    }

    #[test]
    fn a_resumed_agent_runs_again_and_hands_back_an_unseen_report() {
        let work = Work::default();
        let chat = Uuid::new_v4();
        let (id, _) = work.start("alice", chat, WorkKind::Agent, "dig", Delivery::Later);
        work.finish(id, ok("first"));
        assert_eq!(work.resume("bob", chat, id).err(), Some(UnknownWork));
        let (_, unseen) = work.resume("alice", chat, id).expect("its own agent");
        assert_eq!(unseen, Some(ok("first")));
        assert_eq!(work.is_running("alice", chat, id), Ok(true));
        assert_eq!(work.claim(chat), 0, "nothing waits while it runs");
        assert_eq!(work.finish(id, ok("second")), AfterFinish::Deliver);
        assert_eq!(work.claim(chat), 1);
        let claimed = work.take_claimed(chat);
        assert_eq!(claimed[0].outcome, ok("second"));
    }

    #[tokio::test]
    async fn waiting_gives_up_at_its_timeout_and_only_for_the_chats_own_work() {
        let work = Work::default();
        let chat = Uuid::new_v4();
        let (id, _) = work.start("alice", chat, WorkKind::Job, "slow", Delivery::Now);
        assert_eq!(
            work.wait("alice", chat, id, Duration::from_millis(10))
                .await,
            Ok(None)
        );
        assert_eq!(
            work.wait("bob", chat, id, Duration::from_millis(10)).await,
            Err(UnknownWork)
        );
        assert_eq!(
            work.wait("alice", Uuid::new_v4(), id, Duration::from_millis(10))
                .await,
            Err(UnknownWork)
        );
    }

    #[test]
    fn cancel_stops_running_work_only() {
        let work = Work::default();
        let chat = Uuid::new_v4();
        let (id, watch) = work.start("alice", chat, WorkKind::Agent, "dig", Delivery::Now);
        assert_eq!(work.cancel("bob", chat, id), Err(UnknownWork));
        assert_eq!(work.cancel("alice", chat, id), Ok(true));
        assert!(watch.cancel.is_cancelled());
        assert_eq!(
            work.finish(id, ok("stopped")),
            AfterFinish::Nothing,
            "a cancelled run reports nothing"
        );
        assert_eq!(work.cancel("alice", chat, id), Ok(false));
    }

    #[test]
    fn a_later_result_waits_for_the_next_message() {
        let work = Work::default();
        let chat = Uuid::new_v4();
        let (id, _) = work.start("alice", chat, WorkKind::Agent, "dig", Delivery::Later);
        assert_eq!(work.finish(id, ok("found it")), AfterFinish::Nothing);
        assert!(!work.due(chat));
        assert_eq!(work.claim(chat), 1);
        assert_eq!(work.take_claimed(chat).len(), 1);
        assert!(work.take_claimed(chat).is_empty());
    }

    #[test]
    fn listing_shows_running_work_first_with_its_last_step() {
        let work = Work::default();
        let chat = Uuid::new_v4();
        let (done, _) = work.start("alice", chat, WorkKind::Job, "done", Delivery::Now);
        let (running, watch) = work.start("alice", chat, WorkKind::Agent, "dig", Delivery::Now);
        work.start(
            "alice",
            Uuid::new_v4(),
            WorkKind::Agent,
            "other chat",
            Delivery::Now,
        );
        watch.set_step("Searching mail".to_owned());
        work.finish(done, ok("x"));
        let listed = work.list("alice", chat);
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].id, running);
        assert_eq!(listed[0].step, "Searching mail");
        assert_eq!(listed[1].outcome, Some(ok("x")));
        assert_eq!(work.running("alice"), 2);
    }
}
