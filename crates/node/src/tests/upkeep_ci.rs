use super::{
    Ci,
    book::{self, Books},
    check::{Answer, CHECK, QUIET},
    forge::{Forge, Host, Target},
    projects::Project,
    watcher::{POLL, Watcher, World},
};
use crate::machine::elect::elect;
use anyhow::{Result, bail};
use beep_proto::{
    RunState, WorkflowRun,
    node::tools::{ForgeReach, HostReach},
};
use iroh::{EndpointId, SecretKey};
use reqwest::{
    StatusCode,
    header::{HeaderMap, HeaderName, HeaderValue},
};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    future::pending,
    path::Path,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tempfile::TempDir;
use tokio::{
    sync::Notify,
    time::{sleep, timeout},
};

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

fn forgejo() -> Host {
    Host::Forgejo {
        url: "https://git.example.com".into(),
        name: "git.example.com".into(),
    }
}

fn project(machines: &[EndpointId]) -> Project {
    project_on(machines, forgejo())
}

fn project_on(machines: &[EndpointId], host: Host) -> Project {
    Project {
        name: "acme/widget".into(),
        target: Target {
            repo: "acme/widget".into(),
            host,
        },
        machines: machines.iter().copied().collect(),
    }
}

#[test]
fn the_watcher_elects_over_the_projects_machines_by_its_hosts_reach() {
    let machines = ids(3);
    let widget = project(&machines);
    let host = widget.target.host_name();
    let reaches = |reached: bool| {
        let mut said = ForgeReach::default();
        said.hosts.insert(
            host.to_owned(),
            HostReach {
                reached,
                failures: 0,
            },
        );
        said
    };
    // Only the last machine reaches the forge host, so it is elected from
    // the first machine's view; a reach recorded under another host name
    // counts for nothing.
    let peers: BTreeMap<EndpointId, ForgeReach> =
        [(machines[1], reaches(false)), (machines[2], reaches(true))].into();
    let none = HostReach::default();
    assert_eq!(
        elect(&widget.machines, machines[0], none, &peers, host),
        Some(machines[2])
    );
    assert_eq!(
        elect(
            &widget.machines,
            machines[0],
            none,
            &peers,
            "other.example.com"
        ),
        Some(machines[0])
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
    let bytes = books.encode().expect("encoded");
    book::write(&file, &bytes).await.expect("saved");
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
    /// GitHub's next answers to checks; once they run out, 304s.
    answers: Mutex<VecDeque<Answer>>,
    /// The ETag each check sent.
    checks: Mutex<Vec<Option<String>>>,
}

/// An answer to a check with `status` and `headers`.
fn answer(status: u16, headers: &[(&'static str, String)]) -> Answer {
    let mut map = HeaderMap::new();

    for (name, value) in headers {
        map.insert(
            HeaderName::from_static(name),
            HeaderValue::from_str(value).expect("a header value"),
        );
    }
    Answer {
        status: StatusCode::from_u16(status).expect("a status"),
        headers: map,
    }
}

impl StandIn {
    fn answer(&self, answer: Answer) {
        self.answers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push_back(answer);
    }

    fn checks(&self) -> Vec<Option<String>> {
        self.checks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

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

    async fn check(&self, _: &Target, etag: Option<&str>) -> Result<Option<Answer>> {
        self.checks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(etag.map(str::to_owned));
        let next = self
            .answers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front();
        Ok(Some(next.unwrap_or_else(|| answer(304, &[]))))
    }
}

/// A mesh of this machine alone, with the project's checkout on `Host`.
struct Alone(EndpointId, Host);

impl World for Alone {
    fn me(&self) -> EndpointId {
        self.0
    }

    fn projects(&self) -> Vec<Project> {
        vec![project_on(&[self.0], self.1.clone())]
    }

    fn online(&self) -> BTreeSet<EndpointId> {
        BTreeSet::new()
    }

    fn reach_view(&self, _: &BTreeSet<EndpointId>) -> Option<BTreeMap<EndpointId, ForgeReach>> {
        Some(BTreeMap::new())
    }

    async fn changed(&mut self) -> bool {
        pending().await
    }

    async fn pushed(&self, _: &Project) -> Option<SystemTime> {
        None
    }
}

/// A mesh with no pinned project, counting how often it is looked at.
struct Empty(EndpointId, Arc<std::sync::atomic::AtomicUsize>);

impl World for Empty {
    fn me(&self) -> EndpointId {
        self.0
    }

    fn projects(&self) -> Vec<Project> {
        self.1.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Vec::new()
    }

    fn online(&self) -> BTreeSet<EndpointId> {
        BTreeSet::new()
    }

    fn reach_view(&self, _: &BTreeSet<EndpointId>) -> Option<BTreeMap<EndpointId, ForgeReach>> {
        Some(BTreeMap::new())
    }

    async fn changed(&mut self) -> bool {
        pending().await
    }

    async fn pushed(&self, _: &Project) -> Option<SystemTime> {
        None
    }
}

#[tokio::test(start_paused = true)]
async fn a_watcher_holding_nothing_with_no_machine_to_ask_never_wakes() {
    let dir = TempDir::new().expect("a temp dir");
    let host = Arc::new(StandIn::default());
    let looks = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let world = Empty(ids(1)[0], looks.clone());
    let watcher = Watcher::start(
        host.clone(),
        world,
        Arc::new(Ci::default()),
        dir.path().join("ci-runs.json"),
    )
    .await;
    tokio::spawn(watcher.run());
    let count = || looks.load(std::sync::atomic::Ordering::Relaxed);

    polls(1).await;
    let first = count();
    assert!(first > 0);

    tokio::time::sleep(QUIET * 3).await;
    assert_eq!(count(), first);
    assert_eq!(host.lists(), 0);
}

/// Starts a watcher on `host` keeping `file`, and its hub.
async fn watcher(host: &Arc<StandIn>, file: &Path) -> Arc<Ci> {
    watcher_on(host, file, forgejo()).await
}

/// `watcher` with the project on `on`.
async fn watcher_on(host: &Arc<StandIn>, file: &Path, on: Host) -> Arc<Ci> {
    let ci = Arc::new(Ci::default());
    let me = ids(1)[0];
    let world = Alone(me, on);
    let watcher = Watcher::start(host.clone(), world, ci.clone(), file.to_owned()).await;
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

#[tokio::test(start_paused = true)]
async fn a_quiet_project_is_listed_every_quiet_period_and_its_file_left_alone() {
    let dir = TempDir::new().expect("a temp dir");
    let file = dir.path().join("ci-runs.json");
    let host = Arc::new(StandIn::default());
    host.set(vec![run(8, RunState::Failed {}, "someone-else", "push")]);
    let _ci = watcher(&host, &file).await;
    polls(1).await;
    assert_eq!(host.lists(), 1);
    // Nothing goes: no list until the quiet period is over.
    tokio::time::sleep(QUIET - POLL * 2).await;
    assert_eq!(host.lists(), 1);
    polls(2).await;
    assert_eq!(host.lists(), 2);
    // Nothing was told, so nothing was written.
    assert!(!file.exists());
    // A run going: listed every minute.
    host.set(vec![run(9, RunState::Running {}, ME, "push")]);
    tokio::time::sleep(QUIET).await;
    let lists = host.lists();
    polls(3).await;
    assert!(host.lists() >= lists + 3, "{} lists", host.lists());
}

#[tokio::test(start_paused = true)]
async fn a_304_lists_nothing_and_a_200_lists_once_and_keeps_its_etag() {
    let dir = TempDir::new().expect("a temp dir");
    let host = Arc::new(StandIn::default());
    host.set(vec![run(8, RunState::Failed {}, "someone-else", "push")]);
    host.answer(answer(200, &[("etag", "\"a\"".into())]));
    let _ci = watcher_on(&host, &dir.path().join("ci-runs.json"), Host::GitHub).await;
    // The first list comes at once, the first check after `CHECK`: it has
    // no ETag, gets one with a 200, and lists.
    sleep(CHECK + Duration::from_secs(1)).await;
    assert_eq!(host.lists(), 2);
    // 304s list nothing, and each check sends the ETag.
    sleep(CHECK * 4).await;
    assert_eq!(host.lists(), 2);
    let a = Some("\"a\"".to_owned());
    assert_eq!(
        host.checks(),
        [None, a.clone(), a.clone(), a.clone(), a.clone()]
    );
    // The runs change: one list, and the new ETag from then on.
    host.answer(answer(200, &[("etag", "\"b\"".into())]));
    sleep(CHECK * 2).await;
    assert_eq!(host.lists(), 3);
    assert_eq!(host.checks().last(), Some(&Some("\"b\"".into())));
}

#[tokio::test(start_paused = true)]
async fn rate_limit_headers_put_the_next_check_off() {
    let dir = TempDir::new().expect("a temp dir");
    let host = Arc::new(StandIn::default());
    host.set(vec![run(8, RunState::Failed {}, "someone-else", "push")]);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("after 1970")
        .as_secs();
    host.answer(answer(
        403,
        &[
            ("x-ratelimit-remaining", "0".into()),
            // Between two ticks of `CHECK`, so the check after is clear.
            ("x-ratelimit-reset", (now + 30 * 60 + 20).to_string()),
        ],
    ));
    let _ci = watcher_on(&host, &dir.path().join("ci-runs.json"), Host::GitHub).await;
    sleep(CHECK + Duration::from_secs(1)).await;
    assert_eq!(host.checks().len(), 1);
    assert_eq!(host.lists(), 1);
    // Nothing is asked until the limit resets.
    sleep(Duration::from_secs(27 * 60)).await;
    assert_eq!(host.checks().len(), 1);
    sleep(Duration::from_secs(4 * 60)).await;
    assert_eq!(host.checks().len(), 2);
    // Retry-After puts it off too.
    host.answer(answer(429, &[("retry-after", "300".into())]));
    sleep(CHECK).await;
    assert_eq!(host.checks().len(), 3);
    sleep(Duration::from_secs(240)).await;
    assert_eq!(host.checks().len(), 3);
    sleep(Duration::from_secs(90)).await;
    assert_eq!(host.checks().len(), 4);
}
