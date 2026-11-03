//! Live updates for the project page: while someone follows a project, one online node forwards its
//! repository's webhook (`webhook/1`), and each delivery rereads the project's copies. Without
//! admin rights or a capable node, the page keeps polling.
//!
//! One supervisor per followed project holds the node session; it ends a few minutes after the
//! last page leaves or when the project is removed, and the node then removes what it made.

mod owner;
mod session;

use super::watch::Key;
use crate::{
    app::App,
    nodes::Online,
    projects::{
        issues::{HostAndMachines, host_and_machines},
        model::{IssueHost, ProjectId},
    },
    protocol::node::webhook::PollReason,
};
use iroh::EndpointId;
use owner::Candidate;
use session::{Ended, Session};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, MutexGuard, PoisonError, Weak,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{
    sync::{Notify, watch},
    time::sleep,
};
use tokio_util::task::AbortOnDropHandle;

/// How long forwarding stays after the last follower left, so page hopping does not churn webhooks.
const GRACE: Duration = Duration::from_secs(5 * 60);
/// How long after a forward stopped another is started.
const RESTART: Duration = Duration::from_secs(15);
/// How long to wait when no node could forward.
const RETRY: Duration = Duration::from_secs(60);
/// How long a non-admin repository polls before its rights are checked again.
const NOT_ADMIN_RETRY: Duration = Duration::from_secs(30 * 60);

/// Which copies an event has read again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Refresh {
    pub(crate) lists: bool,
    pub(crate) runs: bool,
}

impl Refresh {
    pub(crate) const BOTH: Self = Self {
        lists: true,
        runs: true,
    };

    /// What a host event changes on the page: PRs and issues the lists, CI the runs, a push both.
    pub(crate) fn of_event(event: &str) -> Self {
        let (lists, runs) = match event {
            "push" => (true, true),
            "pull_request"
            | "pull_request_sync"
            | "pull_request_review"
            | "pull_request_review_comment"
            | "pull_request_review_approved"
            | "pull_request_review_rejected"
            | "pull_request_comment"
            | "pull_request_assign"
            | "pull_request_label"
            | "pull_request_milestone"
            | "issues"
            | "issue_assign"
            | "issue_label"
            | "issue_milestone"
            | "issue_comment" => (true, false),
            "check_run" | "check_suite" | "status" | "workflow_run" | "workflow_job"
            | "action_run_failure" | "action_run_success" | "action_run_recover" => (false, true),
            _ => (false, false),
        };
        Self { lists, runs }
    }
}

/// Whether a project's host events reach Beep, as the page shows it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LiveState {
    pub(crate) live: bool,
    /// Why it is not live, for the user; `None` while that is unknown.
    pub(crate) why: Option<String>,
}

impl LiveState {
    fn live() -> Self {
        Self {
            live: true,
            why: None,
        }
    }

    fn polling(why: String) -> Self {
        Self {
            live: false,
            why: Some(why),
        }
    }
}

/// Why a project polls, for the user.
fn why_polling(reason: PollReason, host: &IssueHost) -> String {
    match reason {
        PollReason::NotAdmin => {
            "Live updates need admin rights on the repository; Beep reads it every few minutes."
                .to_owned()
        }
        PollReason::NoAddress => format!(
            "No machine takes webhooks from {host}: set webhook_listen in a machine's node.toml \
             to its private-network address."
        ),
        PollReason::NoTool => {
            "No machine could forward the webhook: gh's webhook extension or teacli is missing \
             or signed out."
                .to_owned()
        }
        _ => "No machine could forward the webhook; Beep reads the host every few minutes."
            .to_owned(),
    }
}

/// What the supervisor does after a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Next {
    /// Poll, and check again after this long: every node would answer the same.
    Poll(Duration),
    /// Pass over this node a while and try another now.
    TryAnother,
    /// Start again on any node after this long.
    Restart(Duration),
}

fn next_after(ended: Ended) -> Next {
    match ended {
        Ended::Refused(reason) if !reason.node_specific() => Next::Poll(NOT_ADMIN_RETRY),
        Ended::Refused(_) => Next::TryAnother,
        Ended::Closed => Next::Restart(RESTART),
    }
}

/// One followed project: its pages, its state and its supervisor.
pub(super) struct Slot {
    followers: AtomicUsize,
    /// Woken when a page starts or stops following.
    moved: Notify,
    state: watch::Sender<LiveState>,
    /// The node forwarding now, or last.
    owner: Mutex<Option<EndpointId>>,
    /// Nodes that could not forward, and when.
    refused: Mutex<HashMap<EndpointId, Instant>>,
    supervisor: Mutex<Option<AbortOnDropHandle<()>>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Default for Slot {
    fn default() -> Self {
        Self {
            followers: AtomicUsize::new(0),
            moved: Notify::new(),
            state: watch::Sender::new(LiveState::default()),
            owner: Mutex::default(),
            refused: Mutex::default(),
            supervisor: Mutex::default(),
        }
    }
}

impl Slot {
    fn set(&self, state: LiveState) {
        self.state.send_if_modified(|now| {
            let changed = *now != state;
            *now = state;
            changed
        });
    }

    /// Finishes once no page followed for `GRACE` straight.
    async fn unwatched(&self) {
        loop {
            let moved = self.moved.notified();
            tokio::pin!(moved);
            moved.as_mut().enable();
            if self.followers.load(Ordering::Relaxed) > 0 {
                moved.await;
                continue;
            }
            tokio::select! {
                () = moved => {}
                () = sleep(GRACE) => {
                    if self.followers.load(Ordering::Relaxed) == 0 {
                        return;
                    }
                }
            }
        }
    }
}

/// A page following a project. Dropping it stops following.
pub(crate) struct Following {
    slot: Arc<Slot>,
    /// Whether the project is live, and each change of it.
    pub(crate) state: watch::Receiver<LiveState>,
}

impl Drop for Following {
    fn drop(&mut self) {
        self.slot.followers.fetch_sub(1, Ordering::Relaxed);
        self.slot.moved.notify_waiters();
    }
}

/// The followed projects, by user and project.
#[derive(Default)]
pub(crate) struct Forwards {
    slots: Mutex<HashMap<Key, Arc<Slot>>>,
}

impl Forwards {
    /// The project's slot with one more follower, and whether its supervisor must start.
    fn join(&self, key: Key) -> (Arc<Slot>, bool) {
        let mut slots = lock(&self.slots);
        let slot = slots.entry(key).or_default().clone();
        slot.followers.fetch_add(1, Ordering::Relaxed);
        slot.moved.notify_waiters();
        let idle = lock(&slot.supervisor)
            .as_ref()
            .is_none_or(|supervisor| supervisor.is_finished());
        (slot, idle)
    }

    /// Follows `project` for a page; the first follower starts forwarding, later ones share it.
    pub(crate) fn follow(&self, app: &Arc<App>, user: &str, project: ProjectId) -> Following {
        let key = Key {
            user: user.to_owned(),
            project,
        };
        let (slot, idle) = self.join(key.clone());
        if idle {
            let supervisor = tokio::spawn(supervise(Arc::downgrade(app), key, slot.clone()));
            *lock(&slot.supervisor) = Some(AbortOnDropHandle::new(supervisor));
        }
        Following {
            state: slot.state.subscribe(),
            slot,
        }
    }

    /// Forgets `slot` under the map's lock when nobody follows it; whether it went.
    fn leave(&self, key: &Key, slot: &Arc<Slot>, always: bool) -> bool {
        let mut slots = lock(&self.slots);
        if !always && slot.followers.load(Ordering::Relaxed) > 0 {
            return false;
        }
        if slots.get(key).is_some_and(|at| Arc::ptr_eq(at, slot)) {
            slots.remove(key);
        }
        true
    }

    #[cfg(test)]
    fn followed(&self) -> usize {
        lock(&self.slots).len()
    }
}

/// What one attempt came to.
enum Step {
    /// The project, its user or the app is gone.
    Stop,
    Wait(Duration),
    Again,
}

/// Keeps one node forwarding the project's webhook while pages follow it.
async fn supervise(app: Weak<App>, key: Key, slot: Arc<Slot>) {
    loop {
        let step = tokio::select! {
            () = slot.unwatched() => None,
            step = attempt(&app, &key, &slot) => Some(step),
        };
        let wait = match step {
            None => {
                if leave(&app, &key, &slot, false) {
                    return;
                }
                continue;
            }
            Some(Step::Stop) => {
                leave(&app, &key, &slot, true);
                return;
            }
            Some(Step::Again) => continue,
            Some(Step::Wait(wait)) => wait,
        };
        tokio::select! {
            () = slot.unwatched() => {
                if leave(&app, &key, &slot, false) {
                    return;
                }
            }
            () = sleep(wait) => {}
        }
    }
}

fn leave(app: &Weak<App>, key: &Key, slot: &Arc<Slot>, always: bool) -> bool {
    match app.upgrade() {
        Some(app) => app.repo_watch.forwards.leave(key, slot, always),
        None => true,
    }
}

/// Picks a node and runs one session with it.
async fn attempt(app: &Weak<App>, key: &Key, slot: &Slot) -> Step {
    let Some(strong) = app.upgrade() else {
        return Step::Stop;
    };
    let Some(user) = strong.user(&key.user) else {
        return Step::Stop;
    };
    let project = match user.project(key.project).await {
        Ok(Some(project)) => project,
        Ok(None) => return Step::Stop,
        Err(e) => {
            log::warn!("webhooks: the project did not read: {e:#}");
            return Step::Wait(RETRY);
        }
    };
    let HostAndMachines { host, machines, .. } = match host_and_machines(&user, &project).await {
        Ok(Some(found)) => found,
        Ok(None) => {
            slot.set(LiveState::polling(
                "This project has no GitHub or Forgejo host.".to_owned(),
            ));
            return Step::Wait(NOT_ADMIN_RETRY);
        }
        Err(e) => {
            log::warn!("webhooks: the project's host is unknown: {e}");
            return Step::Wait(RETRY);
        }
    };
    let online: Vec<Online> = strong
        .nodes
        .of_user(user.id())
        .into_iter()
        .filter(Online::forwards_webhooks)
        .collect();
    let candidates: Vec<Candidate> = online
        .iter()
        .map(|node| Candidate {
            id: node.endpoint(),
            of_project: machines.contains(&node.endpoint()),
        })
        .collect();
    let current = *lock(&slot.owner);
    let picked = owner::pick(&candidates, current, &lock(&slot.refused), Instant::now());
    let Some(node) = picked.and_then(|id| online.into_iter().find(|node| node.endpoint() == id))
    else {
        if slot.state.borrow().why.is_none() || candidates.is_empty() {
            slot.set(LiveState::polling(
                "No machine that can forward webhooks is online; Beep reads the host every few \
                 minutes."
                    .to_owned(),
            ));
        }
        return Step::Wait(RETRY);
    };
    *lock(&slot.owner) = Some(node.endpoint());
    // The app is held weakly while the session lasts.
    drop(strong);
    let session = Session {
        app: app.clone(),
        user: &user,
        project: key.project,
        host: &host,
        repo: &project.name,
        slot,
    };
    let ended = session.run(&node).await;
    if slot.state.borrow().live {
        slot.set(LiveState::default());
    }
    let ended = match ended {
        Ok(ended) => ended,
        Err(e) => {
            log::info!("webhooks: a forward failed: {e:#}");
            lock(&slot.refused).insert(node.endpoint(), Instant::now());
            return Step::Wait(RESTART);
        }
    };
    match (ended, next_after(ended)) {
        (Ended::Refused(reason), Next::Poll(wait)) => {
            slot.set(LiveState::polling(why_polling(reason, &host)));
            Step::Wait(wait)
        }
        (Ended::Refused(reason), _) => {
            lock(&slot.refused).insert(node.endpoint(), Instant::now());
            slot.set(LiveState::polling(why_polling(reason, &host)));
            Step::Again
        }
        (_, Next::Restart(wait) | Next::Poll(wait)) => Step::Wait(wait),
        (_, Next::TryAnother) => Step::Again,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn events_map_to_the_copies_they_change() {
        let lists = Refresh {
            lists: true,
            runs: false,
        };
        let runs = Refresh {
            lists: false,
            runs: true,
        };
        let none = Refresh {
            lists: false,
            runs: false,
        };
        for event in [
            "pull_request",
            "issues",
            "issue_comment",
            "pull_request_review",
            "pull_request_sync",
        ] {
            assert_eq!(Refresh::of_event(event), lists, "{event}");
        }
        for event in [
            "check_run",
            "check_suite",
            "workflow_run",
            "workflow_job",
            "action_run_failure",
        ] {
            assert_eq!(Refresh::of_event(event), runs, "{event}");
        }
        assert_eq!(Refresh::of_event("push"), Refresh::BOTH);
        assert_eq!(Refresh::of_event("ping"), none);
        assert_eq!(Refresh::of_event("star"), none);
    }

    #[test]
    fn a_repository_the_user_does_not_administer_polls_without_trying_other_nodes() {
        assert_eq!(
            next_after(Ended::Refused(PollReason::NotAdmin)),
            Next::Poll(NOT_ADMIN_RETRY)
        );
        let why = why_polling(PollReason::NotAdmin, &IssueHost::Github);
        assert!(why.contains("admin"), "{why}");
        // A reason about one node lets another try.
        for reason in [PollReason::NoTool, PollReason::NoAddress, PollReason::Busy] {
            assert_eq!(next_after(Ended::Refused(reason)), Next::TryAnother);
        }
        assert_eq!(next_after(Ended::Closed), Next::Restart(RESTART));
    }

    #[test]
    fn pages_following_one_project_share_one_supervisor() {
        let forwards = Forwards::default();
        let key = Key {
            user: "alice".into(),
            project: ProjectId(Uuid::from_u128(1)),
        };
        let (first, start_first) = forwards.join(key.clone());
        assert!(start_first);
        *lock(&first.supervisor) = Some(AbortOnDropHandle::new(tokio_test_pending()));
        let (second, start_second) = forwards.join(key.clone());
        assert!(Arc::ptr_eq(&first, &second));
        assert!(!start_second, "one owner per project");
        assert_eq!(first.followers.load(Ordering::Relaxed), 2);
        assert_eq!(forwards.followed(), 1);
        // Still followed: it stays.
        assert!(!forwards.leave(&key, &first, false));
        first.followers.store(0, Ordering::Relaxed);
        assert!(forwards.leave(&key, &first, false));
        assert_eq!(forwards.followed(), 0);
    }

    /// A task that never ends, in a runtime made for the test.
    fn tokio_test_pending() -> tokio::task::JoinHandle<()> {
        RUNTIME.spawn(std::future::pending())
    }

    static RUNTIME: std::sync::LazyLock<tokio::runtime::Runtime> = std::sync::LazyLock::new(|| {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("a runtime")
    });
}
