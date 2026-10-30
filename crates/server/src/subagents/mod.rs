//! Background work: sub-agents and jobs, run outside the user's turns.
//!
//! A sub-agent is a short agent run of some [`Kind`]: its instructions and
//! limits. One run is a [`Spec`]: the kind, the model from the config, the
//! tools it may use, the [`Decision`]s it ends with and its task. Runs go
//! through flashmind's `AgentManager`, which owns the agent loop, its status
//! and its cancellation; [`Subagents::run`] adds what a server needs on top:
//! a token budget, a time limit and a typed decision.
//!
//! A job is one slow tool call the main agent moved to the background
//! (`background run`). Sub-agents the main agent starts (`background spawn`)
//! and jobs are listed together per chat (see [`work`]); when one ends its
//! result goes back into the chat it came from, starting a new turn there
//! once no turn is running.
//!
//! Every call a sub-agent or job makes passes the run's gate
//! (`permissions::Gate`): the user's app rules, the same as in a turn.
//!
//! [`Subagents`] owns every task, bounded per user and per [`Pool`]. Event
//! triage (`crate::events::triage`) and the general agent behind
//! `background spawn` are the kinds so far.

mod approvals;
mod decision;
mod kept;
mod run;
pub(crate) mod tools;
pub(crate) mod work;

pub use approvals::Approvals;
pub use decision::Decision;
pub use run::{Ended, Failure, Watch};

use crate::{app::UserNotice, research::RoleModel};
use flashmind::{
    core::CancellationToken,
    types::{AgentLlmConfig, LlmProvider, ProviderPreferences, ToolGate, tool::Tool},
};
use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};
use tokio::{
    sync::{Notify, broadcast},
    task::JoinSet,
};
use work::Work;

/// What a task is, for its limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Pool {
    /// Event triage runs.
    Triage,
    /// Sub-agents the main agent started.
    Agents,
    /// Tool calls moved to the background.
    Jobs,
    /// Handing results back to chats. Cheap: it waits for a turn to end.
    Deliveries,
}

impl Pool {
    /// Tasks one user may have going in this pool.
    pub const fn per_user(self) -> usize {
        match self {
            Pool::Triage => 2,
            Pool::Agents => 3,
            Pool::Jobs => 4,
            Pool::Deliveries => 16,
        }
    }

    /// Tasks the server may have going in this pool.
    pub const fn total(self) -> usize {
        match self {
            Pool::Triage => 8,
            Pool::Agents => 8,
            Pool::Jobs => 32,
            Pool::Deliveries => 256,
        }
    }
}

/// What a sub-agent is, and how far one run of it may go.
#[derive(Debug)]
pub struct Kind {
    /// For logs, such as `event_triage`.
    pub name: &'static str,
    /// Its system prompt.
    pub instructions: &'static str,
    /// Model calls one run may make.
    pub max_turns: usize,
    /// Prompt and completion tokens one run may use, summed over its calls.
    pub max_tokens: u64,
    /// The longest one run may take.
    pub time_limit: Duration,
}

/// One run of a kind, ending in one of `decisions`.
pub struct Spec<D> {
    pub kind: &'static Kind,
    pub model: RoleModel,
    /// OpenRouter routing for `model`, if the config sets one.
    pub routing: Option<ProviderPreferences>,
    /// Every tool it may call besides its decisions. Nothing else is given.
    pub tools: Vec<Arc<dyn Tool>>,
    pub decisions: Vec<Decision<D>>,
    /// What to do, as the run's one user message.
    pub task: String,
    /// Background the run reads before the task, such as what the user turned on.
    pub context: Option<String>,
    /// Lets a caller follow and stop the run; `None` when nobody does.
    pub watch: Option<Watch>,
    /// Checks every call before it runs; `None` only for runs whose tools
    /// cannot act.
    pub gate: Option<Arc<dyn ToolGate>>,
}

/// No room for another task: the user's or the server's are all busy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("too much background work is running; try again when some of it ends")]
pub struct Busy;

/// Tasks going now, per pool and user and per pool in total.
#[derive(Debug, Default)]
struct Slots {
    per_user: HashMap<(Pool, String), usize>,
    total: HashMap<Pool, usize>,
}

impl Slots {
    fn has_room(&self, pool: Pool, user: &str) -> bool {
        let mine = self
            .per_user
            .get(&(pool, user.to_owned()))
            .copied()
            .unwrap_or(0);
        let all = self.total.get(&pool).copied().unwrap_or(0);
        mine < pool.per_user() && all < pool.total()
    }

    fn take(&mut self, pool: Pool, user: &str) -> Result<(), Busy> {
        if !self.has_room(pool, user) {
            return Err(Busy);
        }
        *self.per_user.entry((pool, user.to_owned())).or_default() += 1;
        *self.total.entry(pool).or_default() += 1;
        Ok(())
    }

    fn give_back(&mut self, pool: Pool, user: &str) {
        let key = (pool, user.to_owned());
        if let Some(mine) = self.per_user.get_mut(&key) {
            *mine = mine.saturating_sub(1);
            if *mine == 0 {
                self.per_user.remove(&key);
            }
        }
        if let Some(all) = self.total.get_mut(&pool) {
            *all = all.saturating_sub(1);
        }
    }

    fn running(&self, pool: Pool, user: &str) -> usize {
        self.per_user
            .get(&(pool, user.to_owned()))
            .copied()
            .unwrap_or(0)
    }
}

/// Gives a task's slot back when it ends, however it ends.
struct Slot {
    slots: Arc<Mutex<Slots>>,
    pool: Pool,
    user: String,
}

impl Drop for Slot {
    fn drop(&mut self) {
        lock(&self.slots).give_back(self.pool, &self.user);
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Every background task on the server, kept on `App`.
pub struct Subagents {
    provider: Arc<dyn LlmProvider>,
    /// The server's settings each run starts from; it sets its own model.
    llm: AgentLlmConfig,
    slots: Arc<Mutex<Slots>>,
    tasks: Mutex<JoinSet<()>>,
    stop: CancellationToken,
    /// Sub-agents and jobs per chat, and results on their way back.
    pub(crate) work: Work,
    /// Calls waiting for the user's OK.
    pub approvals: Arc<Approvals>,
    /// Woken whenever a turn ends, so a waiting delivery tries again.
    pub(crate) turn_ended: Notify,
    /// Sub-agents kept for `background message`.
    pub(crate) kept: kept::KeptAgents<run::Session<tools::Report>>,
}

impl Subagents {
    pub fn new(
        provider: Arc<dyn LlmProvider>,
        llm: AgentLlmConfig,
        notifications: broadcast::Sender<UserNotice>,
    ) -> Self {
        Self {
            approvals: Arc::new(Approvals::new(notifications)),
            provider,
            llm,
            slots: Arc::default(),
            tasks: Mutex::new(JoinSet::new()),
            stop: CancellationToken::new(),
            work: Work::default(),
            turn_ended: Notify::new(),
            kept: kept::KeptAgents::default(),
        }
    }

    /// Starts `work` for `user` in the background, holding one of their
    /// slots in `pool` until it ends. Refused with [`Busy`] when no slot is
    /// free or the server is stopping; a stop drops work still running.
    pub fn spawn<F>(&self, pool: Pool, user: &str, work: F) -> Result<(), Busy>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        if self.stop.is_cancelled() {
            return Err(Busy);
        }
        lock(&self.slots).take(pool, user)?;
        let slot = Slot {
            slots: self.slots.clone(),
            pool,
            user: user.to_owned(),
        };
        let stop = self.stop.clone();
        let mut tasks = lock(&self.tasks);
        // Reap what ended, so the set does not grow and a panic is seen.
        while let Some(ended) = tasks.try_join_next() {
            if let Err(error) = ended
                && error.is_panic()
            {
                log::error!("subagents: a task panicked: {error}");
            }
        }
        tasks.spawn(async move {
            let _slot = slot;
            tokio::select! {
                _ = stop.cancelled() => {}
                _ = work => {}
            }
        });
        Ok(())
    }

    /// How many tasks `user` has going in `pool`.
    pub fn running(&self, pool: Pool, user: &str) -> usize {
        lock(&self.slots).running(pool, user)
    }

    /// Whether `user` could start another task in `pool` now.
    pub fn has_room(&self, pool: Pool, user: &str) -> bool {
        lock(&self.slots).has_room(pool, user)
    }

    /// The results the chat's running turn claimed as it was created,
    /// handed over once.
    pub fn claimed_context(&self, chat: uuid::Uuid) -> Option<String> {
        let claimed = self.work.take_claimed(chat);
        (!claimed.is_empty()).then(|| work::claimed_context(&claimed))
    }

    /// Tells waiting deliveries that a turn ended, so a chat may be free.
    pub fn turn_ended(&self) {
        self.turn_ended.notify_waiters();
    }

    /// Stops every task, as the server shuts down.
    pub fn stop(&self) {
        self.stop.cancel();
        lock(&self.tasks).abort_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_are_bounded_per_user_and_in_total_for_each_pool() {
        let mut slots = Slots::default();
        for _ in 0..Pool::Triage.per_user() {
            assert_eq!(slots.take(Pool::Triage, "alice"), Ok(()));
        }
        assert_eq!(slots.take(Pool::Triage, "alice"), Err(Busy));
        assert_eq!(
            slots.take(Pool::Jobs, "alice"),
            Ok(()),
            "each pool counts on its own"
        );
        slots.give_back(Pool::Triage, "alice");
        assert_eq!(slots.take(Pool::Triage, "alice"), Ok(()));

        let mut slots = Slots::default();
        let users: Vec<String> = (0..Pool::Agents.total())
            .map(|n| format!("user-{n}"))
            .collect();
        for user in &users {
            assert_eq!(slots.take(Pool::Agents, user), Ok(()));
        }
        assert_eq!(slots.take(Pool::Agents, "one-more"), Err(Busy));
        slots.give_back(Pool::Agents, &users[0]);
        assert_eq!(slots.take(Pool::Agents, "one-more"), Ok(()));
        assert_eq!(slots.running(Pool::Agents, &users[0]), 0);
    }
}
