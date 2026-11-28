//! The watch itself: one task per node that owns everything it knows. It
//! elects the watcher of every pinned project each minute and lists the
//! runs of those it holds: every minute while a run goes or after a push
//! from a checkout here, every `QUIET` otherwise, and at once when a GitHub
//! project's check every `CHECK` sees its runs changed. The other machines are
//! followed for how they reach the hosts, which `Reaches` owns, and elected on
//! when one comes or goes or says something new.
//! A mesh change elects again. Going runs are waited on by at most
//! `MAX_WAITS` CLI processes at once, owned here and killed when the role
//! moves away or the node stops. Every call to a forge runs in its own task,
//! never two for one project, and is applied when the loop joins it.

use super::{
    Ci,
    book::{self, Books},
    check::{Answer, CHECK, Checks, QUIET},
    forge::{Cli, Forge, Host, Target},
    projects::{self, Project},
};
use crate::{
    agents::moving::project_folder,
    machine::elect::{Reaches, online},
};
use anyhow::{Result, bail};
use beep_mesh::Mesh;
use beep_proto::{
    RunState, WorkflowRun,
    node::{agent::RunNews, tools::ForgeReach},
};
use futures::future::join_all;
use iroh::EndpointId;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque},
    fs,
    future::Future,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};
use tokio::{
    select,
    sync::watch,
    task::{AbortHandle, Id, JoinError, JoinSet, spawn_blocking},
    time::{Instant, MissedTickBehavior, interval, interval_at, timeout},
};

/// How often a busy project's runs are listed: one with a run going, or
/// pushed to within `BUSY`.
pub(super) const POLL: Duration = Duration::from_secs(60);
/// How long a project stays busy after a push.
const BUSY: Duration = Duration::from_secs(10 * 60);
/// How long a list, a read or a login may take, on top of the CLI's own limit.
const CALL_LIMIT: Duration = Duration::from_secs(45);
/// The most runs waited on at once.
pub(super) const MAX_WAITS: usize = 4;
/// The most runs waiting for a wait; the rest are seen by the next list.
const MAX_QUEUED: usize = 32;
/// The file the told runs are kept in, in the node's folder.
const FILE: &str = "ci-runs.json";

/// What the watch sees of the mesh.
pub(super) trait World: Send + Sync + 'static {
    fn me(&self) -> EndpointId;
    /// The pinned projects.
    fn projects(&self) -> Vec<Project>;
    /// The other members online now.
    fn online(&self) -> BTreeSet<EndpointId>;
    /// Follows `wanted`; idempotent.
    fn track(&self, _wanted: &BTreeSet<EndpointId>) {}
    /// What the machines said, once every one of `wanted` has said something;
    /// none before.
    fn reach_view(&self, wanted: &BTreeSet<EndpointId>)
    -> Option<BTreeMap<EndpointId, ForgeReach>>;
    /// Waits for the mesh to change; false once it is gone.
    fn changed(&mut self) -> impl Future<Output = bool> + Send;
    /// Rises when a machine pushes a different reach; none for a world
    /// whose machines push nothing.
    fn reached(&self) -> Option<watch::Receiver<()>> {
        None
    }
    /// When this machine's checkout of `project` last moved a remote's ref,
    /// as a push does; `None` with no checkout here or no such ref.
    fn pushed(&self, project: &Project) -> impl Future<Output = Option<SystemTime>> + Send;
}

/// Watches while the node is in `mesh`, keeping its file in `dir`.
pub(crate) async fn watch(mesh: Mesh, me: EndpointId, dir: PathBuf, ci: Arc<Ci>, reaches: Reaches) {
    let world = MeshWorld {
        updates: mesh.updates(),
        reaches,
        mesh,
        me,
    };
    Watcher::start(Cli::new(), world, ci, dir.join(FILE))
        .await
        .run()
        .await;
}

/// A run of a project, as waits are kept.
type RunKey = (String, u64);

/// A wait that ended: true when the CLI said the run passed.
struct Waited {
    key: RunKey,
    passed: Result<bool>,
}

/// What a project's next call to its forge is, once the one running ends.
enum Todo {
    List,
    /// Read a run whose wait ended.
    Read(Waited),
}

/// A call to a forge that ended, with the project it was for.
struct Called {
    project: Project,
    outcome: Outcome,
}

enum Outcome {
    /// The cheap check, made at `at`; `None` with no token.
    Checked {
        at: Instant,
        answer: Result<Option<Answer>>,
    },
    /// The account and the runs.
    Listed(Result<(String, Vec<WorkflowRun>)>),
    Read {
        waited: Waited,
        run: Box<Result<WorkflowRun>>,
    },
}

pub(super) struct Watcher<F, W> {
    forge: Arc<F>,
    world: W,
    ci: Arc<Ci>,
    file: PathBuf,
    books: Books,
    /// The account signed in on each host, by host name.
    logins: HashMap<String, String>,
    /// Rises when a machine pushes a different reach.
    reached: Option<watch::Receiver<()>>,
    /// Whether the machines have said something once, so an election had their
    /// reach to go on.
    elected: bool,
    /// The pinned projects, as last read.
    projects: Vec<Project>,
    /// The projects this machine watches.
    held: BTreeSet<String>,
    waits: JoinSet<Waited>,
    /// The calls to forges in flight, and the project each is for.
    calls: JoinSet<Called>,
    calling: HashMap<Id, String>,
    /// What each project does next, after its call in flight.
    pending: HashMap<String, VecDeque<Todo>>,
    /// The runs waited on, and their tasks.
    waiting: HashMap<RunKey, AbortHandle>,
    tasks: HashMap<Id, RunKey>,
    queued: VecDeque<RunKey>,
    /// The last read of each run waited on or queued.
    last: HashMap<RunKey, WorkflowRun>,
    /// When each held project is listed next.
    next: HashMap<String, Instant>,
    /// Each project's last push seen here, and until when it is busy.
    pushes: HashMap<String, SystemTime>,
    busy_until: HashMap<String, Instant>,
    /// Each held GitHub project's checks.
    checks: HashMap<String, Checks>,
    /// The file's bytes as last written or read.
    saved: Option<Vec<u8>>,
}

impl<F: Forge, W: World> Watcher<F, W> {
    pub(super) async fn start(forge: F, world: W, ci: Arc<Ci>, file: PathBuf) -> Self {
        let books = Books::load(&file).await;
        Self {
            reached: world.reached(),
            forge: Arc::new(forge),
            world,
            ci,
            saved: books.encode().ok(),
            books,
            file,
            logins: HashMap::new(),
            elected: false,
            projects: Vec::new(),
            held: BTreeSet::new(),
            waits: JoinSet::new(),
            calls: JoinSet::new(),
            calling: HashMap::new(),
            pending: HashMap::new(),
            waiting: HashMap::new(),
            tasks: HashMap::new(),
            queued: VecDeque::new(),
            last: HashMap::new(),
            next: HashMap::new(),
            pushes: HashMap::new(),
            busy_until: HashMap::new(),
            checks: HashMap::new(),
        }
    }

    pub(super) async fn run(mut self) {
        let mut tick = interval(POLL);
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        // The first list comes with the first tick; the first check after.
        let mut check = interval_at(Instant::now() + CHECK, CHECK);
        check.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            let parked = self.held.is_empty() && self.elected;

            select! {
                _ = check.tick(), if !parked => self.check_due(),
                _ = tick.tick(), if !parked => self.poll().await,
                () = async {
                    if let Some(reached) = &mut self.reached
                        && reached.changed().await.is_ok()
                    {
                        return;
                    }

                    std::future::pending::<()>().await;
                } => {
                    // A machine said how it reaches the hosts: elect on it.
                    self.reelect();
                }
                alive = self.world.changed() => {
                    if !alive {
                        return;
                    }
                    // A machine came or went: its reach decides the watcher.
                    self.reelect();
                }
                Some(done) = self.waits.join_next_with_id(), if !self.waits.is_empty() => {
                    self.waited(done);
                }
                Some(done) = self.calls.join_next_with_id(), if !self.calls.is_empty() => {
                    self.applied(done).await;
                }
            }
        }
    }

    /// Follows the machines, elects the watchers once they have said how they
    /// reach the hosts, and lists the held projects that are due.
    async fn poll(&mut self) {
        // The first election waits for the first answers: another machine may be the better one.
        let Some(peers) = self.peers() else {
            return;
        };

        self.elect(peers);
        self.list_due().await;
    }

    /// Elects again on what the machines say, and starts the listing of what
    /// this machine took on.
    fn reelect(&mut self) {
        let Some(peers) = self.peers() else {
            return;
        };

        for project in self.elect(peers) {
            self.begin(project, Todo::List);
        }
    }

    /// What the online machines with a checkout of a pinned project said of
    /// their reach, once all have said something.
    fn peers(&mut self) -> Option<BTreeMap<EndpointId, ForgeReach>> {
        let me = self.world.me();
        let online = self.world.online();
        let wanted: BTreeSet<EndpointId> = self
            .world
            .projects()
            .iter()
            .flat_map(|project| project.machines.iter().copied())
            .filter(|machine| *machine != me && online.contains(machine))
            .collect();

        self.world.track(&wanted);
        let peers = self.world.reach_view(&wanted)?;
        self.elected = true;

        Some(peers)
    }

    /// Elects the watcher of every pinned project; the projects this machine
    /// took on now. What it lost stops at once.
    fn elect(&mut self, mut peers: BTreeMap<EndpointId, ForgeReach>) -> Vec<Project> {
        let me = self.world.me();
        let online = self.world.online();
        self.projects = self.world.projects();
        peers.retain(|peer, _| online.contains(peer));
        let held: BTreeSet<String> = self
            .projects
            .iter()
            .filter(|project| project.elected(me, &self.ci, &peers) == Some(me))
            .map(|project| project.name.clone())
            .collect();
        let lost: Vec<String> = self.held.difference(&held).cloned().collect();

        for lost in lost {
            log::info!("CI watch: {lost} is watched elsewhere now");
            self.drop_waits(&lost);
            self.pending.remove(&lost);
        }

        let gained: Vec<Project> = self
            .projects
            .iter()
            .filter(|project| held.contains(&project.name) && !self.held.contains(&project.name))
            .cloned()
            .collect();
        for project in &gained {
            log::info!("CI watch: watching {} here", project.name);
        }

        self.held = held;
        let pinned: BTreeSet<&str> = self.projects.iter().map(|p| p.name.as_str()).collect();
        self.books.retain(|name| pinned.contains(name));
        let held = &self.held;
        self.next.retain(|name, _| held.contains(name));
        self.pushes.retain(|name, _| pinned.contains(name.as_str()));
        self.busy_until.retain(|name, _| held.contains(name));
        self.checks.retain(|name, _| held.contains(name));
        gained
    }

    /// Checks every held GitHub project that is due, and lists those whose
    /// runs changed. With no token a project is not checked, and its lists
    /// keep their schedule.
    fn check_due(&mut self) {
        let held: Vec<Project> = self
            .held_projects()
            .filter(|project| project.target.host == Host::GitHub)
            .collect();

        for project in held {
            let at = Instant::now();
            let busy = self.calling(&project.name);
            let checks = self.checks.entry(project.name.clone()).or_default();

            if !checks.due(at) || busy {
                continue;
            }

            let etag = checks.etag().map(str::to_owned);
            let forge = self.forge.clone();
            self.spawn(project.clone(), async move {
                let answer = bounded(forge.check(&project.target, etag.as_deref())).await;
                Called {
                    project,
                    outcome: Outcome::Checked { at, answer },
                }
            });
        }
    }

    /// Lists every held project that is due: busy ones every `POLL`, the
    /// others every `QUIET`. A push seen here makes a project busy and due.
    async fn list_due(&mut self) {
        let now = Instant::now();
        let held: Vec<Project> = self.held_projects().collect();
        let world = &self.world;
        let pushed = join_all(held.iter().map(|project| world.pushed(project))).await;

        for (project, pushed) in held.into_iter().zip(pushed) {
            if let Some(at) = pushed {
                let before = self.pushes.insert(project.name.clone(), at);
                if before.is_some_and(|before| before != at) {
                    self.busy_until.insert(project.name.clone(), now + BUSY);
                    self.next.remove(&project.name);
                }
            }

            if self.next.get(&project.name).is_none_or(|next| now >= *next) {
                self.begin(project, Todo::List);
            }
        }
    }

    fn held_projects(&self) -> impl Iterator<Item = Project> + '_ {
        self.projects
            .iter()
            .filter(|project| self.held.contains(&project.name))
            .cloned()
    }

    /// Whether a call for `project` is in flight.
    fn calling(&self, project: &str) -> bool {
        self.calling.values().any(|name| name == project)
    }

    fn spawn(&mut self, project: Project, call: impl Future<Output = Called> + Send + 'static) {
        let handle = self.calls.spawn(call);
        self.calling.insert(handle.id(), project.name);
    }

    /// Does `todo` for `project` now, or after the call it has in flight.
    fn begin(&mut self, project: Project, todo: Todo) {
        if self.calling(&project.name) {
            let queue = self.pending.entry(project.name).or_default();

            if !matches!(todo, Todo::List) || !queue.iter().any(|todo| matches!(todo, Todo::List)) {
                queue.push_back(todo);
            }

            return;
        }

        let forge = self.forge.clone();

        match todo {
            Todo::List => {
                let login = self.logins.get(project.target.host_name()).cloned();
                self.spawn(project.clone(), async move {
                    let listed = async {
                        let login = match login {
                            Some(login) => login,
                            None => bounded(forge.login(&project.target)).await?,
                        };
                        let runs = bounded(forge.list(&project.target)).await?;
                        Ok::<_, anyhow::Error>((login, runs))
                    }
                    .await;
                    Called {
                        project,
                        outcome: Outcome::Listed(listed),
                    }
                });
            }
            Todo::Read(waited) => {
                self.spawn(project.clone(), async move {
                    let run = bounded(forge.one(&project.target, waited.key.1)).await;
                    Called {
                        project,
                        outcome: Outcome::Read {
                            waited,
                            run: Box::new(run),
                        },
                    }
                });
            }
        }
    }

    /// Starts what `project` had waiting for its call to end.
    fn resume(&mut self, project: &str) {
        let Some(todo) = self.pending.get_mut(project).and_then(VecDeque::pop_front) else {
            return;
        };

        let held = self.held_projects().find(|held| held.name == project);

        if let Some(project) = held {
            self.begin(project, todo);
        }
    }

    /// A call ended: its answer is applied here and the file is written
    /// when it changed. Another project's call can stay in flight for long.
    async fn applied(&mut self, done: Result<(Id, Called), JoinError>) {
        let called = match done {
            Ok((id, called)) => {
                self.calling.remove(&id);
                called
            }
            Err(e) => {
                log::warn!("CI watch: a call to a host stopped: {e}");

                if let Some(project) = self.calling.remove(&e.id()) {
                    self.resume(&project);
                }

                return;
            }
        };
        let Called { project, outcome } = called;

        // The role may have moved while the call ran.
        if self.held.contains(&project.name) {
            match outcome {
                Outcome::Checked { at, answer } => self.checked(&project, at, answer),
                Outcome::Listed(listed) => self.listed(&project, listed),
                Outcome::Read { waited, run } => self.read(&project, waited, *run),
            }
        }

        self.resume(&project.name);
        self.save().await;
    }

    fn checked(&mut self, project: &Project, at: Instant, answer: Result<Option<Answer>>) {
        let answer = match answer {
            Ok(Some(answer)) => Ok(answer),
            Ok(None) => return,
            Err(e) => Err(e),
        };
        let checks = self.checks.entry(project.name.clone()).or_default();

        if checks.answered(answer, at, SystemTime::now()) {
            self.begin(project.clone(), Todo::List);
        }
    }

    /// One list of `project`'s runs: what ended is told, what goes is waited on.
    fn listed(&mut self, project: &Project, listed: Result<(String, Vec<WorkflowRun>)>) {
        let host = project.target.host_name().to_owned();
        let (login, runs) = match listed {
            Ok(listed) => listed,
            Err(e) => {
                self.ci.record_reach(&host, false);
                self.logins.remove(&host);
                log::info!("CI watch: the runs of {} did not read: {e:#}", project.name);
                return;
            }
        };
        self.ci.record_reach(&host, true);
        self.logins.insert(host, login.clone());
        let going = runs.iter().any(|run| run.state.going());
        let busy = going
            || self
                .busy_until
                .get(&project.name)
                .is_some_and(|until| Instant::now() < *until);
        let wait = if busy { POLL } else { QUIET };
        self.next
            .insert(project.name.clone(), Instant::now() + wait);
        let seen = self
            .books
            .listed(&project.name, &runs, &login, SystemTime::now());
        for run in seen.tell {
            self.tell(project, run);
        }

        for run in seen.going {
            self.queue(project, run);
        }
    }

    fn tell(&mut self, project: &Project, run: WorkflowRun) {
        let key = (project.name.clone(), run.id);
        self.last.remove(&key);
        log::info!(
            "CI watch: run {} of {} ended: {}",
            run.id,
            project.name,
            <&str>::from(run.state)
        );
        self.ci.runs.record(RunNews {
            project: project.name.clone(),
            host: project.target.host_name().to_owned(),
            run,
        });
    }

    /// Waits on going `run`, now or once a wait is free.
    fn queue(&mut self, project: &Project, run: WorkflowRun) {
        let key = (project.name.clone(), run.id);
        self.last.insert(key.clone(), run);

        if self.waiting.contains_key(&key) || self.queued.contains(&key) {
            return;
        }

        if self.waiting.len() < MAX_WAITS {
            self.start_wait(key, project.target.clone());
        } else if self.queued.len() < MAX_QUEUED {
            self.queued.push_back(key);
        }
    }

    fn start_wait(&mut self, key: RunKey, target: Target) {
        let forge = self.forge.clone();
        let id = key.1;
        let task_key = key.clone();
        let handle = self.waits.spawn(async move {
            let passed = forge.wait(&target, id).await;
            Waited {
                key: task_key,
                passed,
            }
        });
        self.tasks.insert(handle.id(), key.clone());
        self.waiting.insert(key, handle);
    }

    /// Starts the queued waits there is room for.
    fn start_queued(&mut self) {
        while self.waiting.len() < MAX_WAITS {
            let Some(key) = self.queued.pop_front() else {
                return;
            };
            let target = self
                .projects
                .iter()
                .find(|project| project.name == key.0 && self.held.contains(&project.name))
                .map(|project| project.target.clone());
            if let Some(target) = target {
                self.start_wait(key, target);
            }
        }
    }

    /// Stops every wait on `project`'s runs.
    fn drop_waits(&mut self, project: &str) {
        self.waiting.retain(|key, task| {
            let stays = key.0 != project;

            if !stays {
                task.abort();
                self.tasks.remove(&task.id());
            }

            stays
        });
        self.queued.retain(|key| key.0 != project);
        self.last.retain(|key, _| key.0 != project);
    }

    /// A wait ended: the run is read once, and told when it ended.
    fn waited(&mut self, done: Result<(Id, Waited), JoinError>) {
        let waited = match done {
            Ok((id, waited)) => {
                self.tasks.remove(&id);
                waited
            }
            Err(e) => {
                // Stopped with its project's role, or it panicked.
                if let Some(key) = self.tasks.remove(&e.id()) {
                    self.waiting.remove(&key);
                }

                self.start_queued();
                return;
            }
        };
        self.waiting.remove(&waited.key);
        self.start_queued();

        let project = self.held_projects().find(|p| p.name == waited.key.0);

        if let Some(project) = project {
            self.begin(project, Todo::Read(waited));
        }
    }

    /// The run of a wait that ended, read once.
    fn read(&mut self, project: &Project, waited: Waited, run: Result<WorkflowRun>) {
        let host = project.target.host_name().to_owned();
        let run = match run {
            Ok(run) => {
                self.ci.record_reach(&host, true);
                Some(run)
            }
            Err(e) => {
                self.ci.record_reach(&host, false);
                log::info!("CI watch: run {} did not read: {e:#}", waited.key.1);
                // The CLI said it passed: that is told; anything else waits
                // for the next list.
                match waited.passed {
                    Ok(true) => self.last.get(&waited.key).cloned().map(|mut run| {
                        run.state = RunState::Passed {};
                        run.finished_at = Some(SystemTime::now());
                        run
                    }),
                    _ => None,
                }
            }
        };
        let Some(run) = run else {
            return;
        };

        if self.books.settle(&project.name, &run, SystemTime::now()) {
            self.tell(project, run);
        }
    }

    /// Writes the file when what it holds changed.
    async fn save(&mut self) {
        let bytes = match self.books.encode() {
            Ok(bytes) => bytes,
            Err(e) => {
                log::warn!("CI watch: {e:#}");
                return;
            }
        };

        if self.saved.as_ref() == Some(&bytes) {
            return;
        }

        match book::write(&self.file, &bytes).await {
            Ok(()) => self.saved = Some(bytes),
            Err(e) => log::warn!("CI watch: {e:#}"),
        }
    }
}

/// `call`, given `CALL_LIMIT`.
async fn bounded<T>(call: impl Future<Output = Result<T>>) -> Result<T> {
    match timeout(CALL_LIMIT, call).await {
        Ok(result) => result,
        Err(_) => bail!("the host did not answer in time"),
    }
}

/// The mesh as the watch sees it.
struct MeshWorld {
    mesh: Mesh,
    me: EndpointId,
    updates: watch::Receiver<u64>,
    reaches: Reaches,
}

impl World for MeshWorld {
    fn me(&self) -> EndpointId {
        self.me
    }

    fn projects(&self) -> Vec<Project> {
        projects::pinned(&self.mesh)
    }

    fn online(&self) -> BTreeSet<EndpointId> {
        online(&self.mesh)
    }

    fn track(&self, wanted: &BTreeSet<EndpointId>) {
        self.reaches.track(wanted.iter().copied());
    }

    fn reach_view(
        &self,
        wanted: &BTreeSet<EndpointId>,
    ) -> Option<BTreeMap<EndpointId, ForgeReach>> {
        self.reaches.settled(wanted).then(|| self.reaches.view())
    }

    fn reached(&self) -> Option<watch::Receiver<()>> {
        Some(self.reaches.changes())
    }

    async fn changed(&mut self) -> bool {
        self.updates.changed().await.is_ok()
    }

    async fn pushed(&self, project: &Project) -> Option<SystemTime> {
        let folder = project_folder(&self.mesh, &project.name, &self.me)?;
        spawn_blocking(move || remote_refs_changed(Path::new(&folder)))
            .await
            .ok()
            .flatten()
    }
}

/// When a remote's ref in the checkout at `folder` last moved: a push (or a
/// fetch) writes them. A few file times, no git. `None` for a linked
/// worktree, whose refs live in another folder.
fn remote_refs_changed(folder: &Path) -> Option<SystemTime> {
    let git = folder.join(".git");
    let modified = |path: &Path| fs::metadata(path).and_then(|meta| meta.modified()).ok();
    let mut newest = modified(&git.join("packed-refs"));
    let mut dirs = vec![git.join("refs/remotes")];

    while let Some(dir) = dirs.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };

        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                dirs.push(entry.path());
            } else if let Some(at) = modified(&entry.path()) {
                newest = newest.max(Some(at));
            }
        }
    }

    newest
}

#[cfg(test)]
#[path = "../../tests/upkeep_ci_watcher.rs"]
mod tests;
