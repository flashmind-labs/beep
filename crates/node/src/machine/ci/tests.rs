use super::{
    Ci,
    book::Books,
    forge::{Forge, Host, Target},
    projects::Project,
    watcher::{POLL, Watcher, World, watcher_of},
};
use anyhow::{Result, bail};
use beep_proto::{
    RunState, WorkflowRun,
    node::tools::{ForgeReach, HostReach},
};
use iroh::{EndpointId, SecretKey};
use std::{
    collections::{BTreeMap, BTreeSet},
    future::pending,
    path::Path,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, SystemTime},
};
use tempfile::TempDir;
use tokio::{sync::Notify, time::timeout};

const ME: &str = "someone";

fn run(id: u64, state: RunState, actor: &str, event: &str) -> WorkflowRun {
    WorkflowRun {
        id,
        number: id,
        workflow: "build.yml".into(),
        title: "Cut the cache warm-up".into(),
        branch: "main".into(),
        event: event.into(),
        actor: actor.into(),
        state,
        url: format!("https://git.example.com/acme/widget/actions/runs/{id}"),
        started_at: None,
        finished_at: state.finished().then(SystemTime::now),
        progress: None,
        pull_requests: Vec::new(),
    }
}

fn ids(n: usize) -> Vec<EndpointId> {
    let mut ids: Vec<EndpointId> = (0..n).map(|_| SecretKey::generate().public()).collect();
    ids.sort();
    ids
}

fn project(machines: &[EndpointId]) -> Project {
    Project {
        name: "acme/widget".into(),
        target: Target {
            repo: "acme/widget".into(),
            host: Host::Forgejo {
                url: "https://git.example.com".into(),
                name: "git.example.com".into(),
            },
        },
        machines: machines.iter().copied().collect(),
    }
}

fn reach(reached: bool, failures: u32) -> HostReach {
    HostReach { reached, failures }
}

/// The watcher each of `reaches`' machines elects, seeing the `online` others.
fn elected(
    project: &Project,
    reaches: &BTreeMap<EndpointId, HostReach>,
    online: &BTreeSet<EndpointId>,
) -> BTreeSet<Option<EndpointId>> {
    let host = project.target.host_name();
    online
        .iter()
        .map(|me| {
            let peers = online
                .iter()
                .filter(|peer| *peer != me)
                .map(|peer| {
                    let mut said = ForgeReach::default();
                    said.hosts.insert(host.to_owned(), reaches[peer]);
                    (*peer, said)
                })
                .collect();
            watcher_of(project, *me, reaches[me], &peers)
        })
        .collect()
}

#[test]
fn every_machine_elects_the_same_one_and_it_moves_when_that_one_goes() {
    let machines = ids(3);
    let widget = project(&machines);
    let mut reaches: BTreeMap<EndpointId, HostReach> =
        machines.iter().map(|id| (*id, reach(false, 0))).collect();
    let mut online: BTreeSet<EndpointId> = machines.iter().copied().collect();
    // Nobody tried the host yet: the smallest id, on every machine.
    assert_eq!(
        elected(&widget, &reaches, &online),
        [Some(machines[0])].into()
    );
    // The first goes offline: the others both move to the next.
    online.remove(&machines[0]);
    assert_eq!(
        elected(&widget, &reaches, &online),
        [Some(machines[1])].into()
    );
    // A machine that reaches the host comes before one never tried.
    online.insert(machines[0]);
    reaches.insert(machines[2], reach(true, 0));
    assert_eq!(
        elected(&widget, &reaches, &online),
        [Some(machines[2])].into()
    );
    // It fails once and keeps the role; three in a row and it gives it up.
    reaches.insert(machines[2], reach(true, 1));
    assert_eq!(
        elected(&widget, &reaches, &online),
        [Some(machines[2])].into()
    );
    reaches.insert(machines[2], reach(true, HostReach::GIVE_UP));
    assert_eq!(
        elected(&widget, &reaches, &online),
        [Some(machines[0])].into()
    );
    // A machine without the checkout never watches.
    let elsewhere = project(&machines[1..]);
    reaches.insert(machines[0], reach(true, 0));
    assert_eq!(
        elected(&elsewhere, &reaches, &online),
        [Some(machines[1])].into()
    );
}

#[tokio::test]
async fn a_run_seen_going_then_failed_is_told_once_even_after_a_restart() {
    let dir = TempDir::new().expect("a temp dir");
    let file = dir.path().join("ci-runs.json");
    let now = SystemTime::now();
    let mut books = Books::load(&file).await;
    let going = [run(7, RunState::Running {}, ME, "push")];
    let seen = books.listed("acme/widget", &going, ME, now);
    assert!(seen.tell.is_empty());
    assert_eq!(seen.going.len(), 1);
    let failed = [run(7, RunState::Failed {}, ME, "push")];
    let seen = books.listed("acme/widget", &failed, ME, now);
    assert_eq!(seen.tell.len(), 1);
    assert!(
        books
            .listed("acme/widget", &failed, ME, now)
            .tell
            .is_empty()
    );
    books.save(&file).await.expect("saved");
    let mut again = Books::load(&file).await;
    assert_eq!(again, books);
    assert!(
        again
            .listed("acme/widget", &failed, ME, now)
            .tell
            .is_empty()
    );
}

#[tokio::test]
async fn only_the_users_pushes_and_pull_requests_are_told_and_old_ones_never() {
    let now = SystemTime::now();
    let mut books = Books::load(Path::new("/nonexistent/ci-runs.json")).await;
    let mut old = run(5, RunState::Passed {}, ME, "push");
    old.finished_at = now.checked_sub(Duration::from_secs(3600));
    let runs = [
        run(1, RunState::Failed {}, "someone-else", "push"),
        run(2, RunState::Failed {}, ME, "schedule"),
        run(3, RunState::Passed {}, "SOMEONE", "pull_request"),
        // An older Forgejo names nobody: the user's.
        run(4, RunState::Cancelled {}, "", "push"),
        old,
    ];
    let told: Vec<u64> = books
        .listed("acme/widget", &runs, ME, now)
        .tell
        .iter()
        .map(|run| run.id)
        .collect();
    assert_eq!(told, [3, 4]);
}

/// A host whose runs the test sets, and whose waits end when it says.
#[derive(Default)]
struct StandIn {
    runs: Mutex<Vec<WorkflowRun>>,
    lists: Mutex<u32>,
    ended: Notify,
}

impl StandIn {
    fn set(&self, runs: Vec<WorkflowRun>) {
        *self.runs.lock().unwrap_or_else(PoisonError::into_inner) = runs;
    }

    fn lists(&self) -> u32 {
        *self.lists.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Forge for Arc<StandIn> {
    async fn list(&self, _: &Target) -> Result<Vec<WorkflowRun>> {
        *self.lists.lock().unwrap_or_else(PoisonError::into_inner) += 1;
        Ok(self
            .runs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone())
    }

    async fn one(&self, _: &Target, id: u64) -> Result<WorkflowRun> {
        let runs = self.runs.lock().unwrap_or_else(PoisonError::into_inner);
        match runs.iter().find(|run| run.id == id) {
            Some(run) => Ok(run.clone()),
            None => bail!("no run {id}"),
        }
    }

    async fn login(&self, _: &Target) -> Result<String> {
        Ok(ME.into())
    }

    async fn wait(&self, _: &Target, _: u64) -> Result<bool> {
        self.ended.notified().await;
        Ok(false)
    }
}

/// A mesh of this machine alone, with the project's checkout.
struct Alone(EndpointId);

impl World for Alone {
    fn me(&self) -> EndpointId {
        self.0
    }

    fn projects(&self) -> Vec<Project> {
        vec![project(&[self.0])]
    }

    fn online(&self) -> BTreeSet<EndpointId> {
        BTreeSet::new()
    }

    async fn ask(&self, _: EndpointId) -> Result<ForgeReach> {
        bail!("nobody else")
    }

    async fn changed(&mut self) -> bool {
        pending().await
    }
}

/// Starts a watcher on `host` keeping `file`, and its hub.
async fn watcher(host: &Arc<StandIn>, file: &Path) -> Arc<Ci> {
    let ci = Arc::new(Ci::default());
    let me = ids(1)[0];
    let watcher = Watcher::start(host.clone(), Alone(me), ci.clone(), file.to_owned()).await;
    tokio::spawn(watcher.run());
    ci
}

/// Lets the watcher run for `polls` minutes of the paused clock.
async fn polls(polls: u32) {
    tokio::time::sleep(POLL * polls).await;
}

#[tokio::test(start_paused = true)]
async fn the_watcher_tells_a_run_once_when_its_wait_ends_and_not_after_a_restart() {
    let dir = TempDir::new().expect("a temp dir");
    let file = dir.path().join("ci-runs.json");
    let host = Arc::new(StandIn::default());
    host.set(vec![
        run(7, RunState::Running {}, ME, "push"),
        run(8, RunState::Failed {}, "someone-else", "push"),
    ]);
    let ci = watcher(&host, &file).await;
    let follower = ids(1)[0];
    let follow = ci.runs.follow(follower);
    polls(1).await;
    assert!(host.lists() >= 1);
    // The run ends; its wait returns and the one read tells it.
    host.set(vec![
        run(7, RunState::Failed {}, ME, "push"),
        run(8, RunState::Failed {}, "someone-else", "push"),
    ]);
    host.ended.notify_waiters();
    let news = timeout(POLL, follow.next()).await.expect("the run is told");
    follow.sent();
    assert_eq!((news.run.id, news.run.state), (7, RunState::Failed {}));
    assert_eq!(news.host, "git.example.com");
    // Later lists see it failed again, and the other actor's run: nothing.
    assert!(timeout(POLL * 3, follow.next()).await.is_err());
    // A restarted node tells it no more, though it ended just now.
    let ci = watcher(&host, &file).await;
    let follow = ci.runs.follow(follower);
    assert!(timeout(POLL * 3, follow.next()).await.is_err());
}
