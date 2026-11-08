//! The watch itself: one task per node that owns everything it knows. It
//! elects the watcher of every pinned project each minute and lists the
//! runs of those it holds: every minute while a run goes or after a push
//! from a checkout here, every `QUIET` otherwise, and at once when a GitHub
//! project's check every `CHECK` sees its runs changed. The other machines are
//! asked how they reach the hosts every `QUIET`, and when one comes or goes.
//! A mesh change elects again. Going runs are waited on by at most
//! `MAX_WAITS` CLI processes at once, owned here and killed when the role
//! moves away or the node stops.

use super::{
    Ci,
    book::{self, Books},
    check::{CHECK, Checks},
    elect::elect,
    forge::{Cli, Forge, Host, Target},
    projects::{self, Project},
};
use crate::agents::{mcp::over_the_mesh, project_folder};
use anyhow::{Result, anyhow, bail};
use beep_mesh::Mesh;
use beep_proto::{
    RunState, WorkflowRun,
    node::{
        agent::RunNews,
        tools::{ForgeReach, HostReach, Reply, Request, Tool},
    },
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
/// How often a quiet project's runs are listed, and the machines asked.
pub(super) const QUIET: Duration = Duration::from_secs(10 * 60);
/// How long a project stays busy after a push.
const BUSY: Duration = Duration::from_secs(10 * 60);
/// How long a list, a read or a login may take, on top of the CLI's own limit.
const CALL_LIMIT: Duration = Duration::from_secs(45);
/// How long another machine has to say how it reaches the hosts.
const ASK_LIMIT: Duration = Duration::from_secs(10);
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
    /// How `peer` reaches the hosts; an error when it cannot say, as a node
    /// from before the watch, which then never watches.
    fn ask(&self, peer: EndpointId) -> impl Future<Output = Result<ForgeReach>> + Send;
    /// Waits for the mesh to change; false once it is gone.
    fn changed(&mut self) -> impl Future<Output = bool> + Send;
    /// When this machine's checkout of `project` last moved a remote's ref,
    /// as a push does; `None` with no checkout here or no such ref.
    fn pushed(&self, project: &Project) -> impl Future<Output = Option<SystemTime>> + Send;
}

/// Watches while the node is in `mesh`, keeping its file in `dir`.
pub(crate) async fn watch(mesh: Mesh, me: EndpointId, dir: PathBuf, ci: Arc<Ci>) {
    let world = MeshWorld {
        updates: mesh.updates(),
        mesh,
        me,
    };
    Watcher::start(Cli::new(), world, ci, dir.join(FILE))
        .await
        .run()
        .await;
}

/// The machine that watches `project`, as this node sees it: `reach` is how
/// this machine reaches the host, `peers` what the online machines that
/// answered said.
pub(super) fn watcher_of(
    project: &Project,
    me: EndpointId,
    reach: HostReach,
    peers: &BTreeMap<EndpointId, ForgeReach>,
) -> Option<EndpointId> {
    let host = project.target.host_name();
    let candidates = project.machines.iter().filter_map(|machine| {
        if *machine == me {
            return Some((me, reach));
        }
        let said = peers.get(machine)?;
        Some((*machine, said.hosts.get(host).copied().unwrap_or_default()))
    });
    elect(candidates)
}

/// A run of a project, as waits are kept.
type RunKey = (String, u64);

/// A wait that ended: true when the CLI said the run passed.
struct Waited {
    key: RunKey,
    passed: Result<bool>,
}

pub(super) struct Watcher<F, W> {
    forge: Arc<F>,
    world: W,
    ci: Arc<Ci>,
    file: PathBuf,
    books: Books,
    /// The account signed in on each host, by host name.
    logins: HashMap<String, String>,
    /// What the online machines said of their reach, this minute.
    peers: BTreeMap<EndpointId, ForgeReach>,
    /// The pinned projects, as last read.
    projects: Vec<Project>,
    /// The projects this machine watches.
    held: BTreeSet<String>,
    waits: JoinSet<Waited>,
    /// The runs waited on, and their tasks.
    waiting: HashMap<RunKey, AbortHandle>,
    tasks: HashMap<Id, RunKey>,
    queued: VecDeque<RunKey>,
    /// The last read of each run waited on or queued.
    last: HashMap<RunKey, WorkflowRun>,
    /// When the machines were last asked, and which were online then.
    asked: Option<(Instant, BTreeSet<EndpointId>)>,
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
            forge: Arc::new(forge),
            world,
            ci,
            saved: books.encode().ok(),
            books,
            file,
            logins: HashMap::new(),
            peers: BTreeMap::new(),
            projects: Vec::new(),
            held: BTreeSet::new(),
            waits: JoinSet::new(),
            waiting: HashMap::new(),
            tasks: HashMap::new(),
            queued: VecDeque::new(),
            last: HashMap::new(),
            asked: None,
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
            select! {
                _ = check.tick() => self.check_due().await,
                _ = tick.tick() => {
                    let due = self
                        .asked
                        .as_ref()
                        .is_none_or(|(at, _)| at.elapsed() >= QUIET);
                    if due {
                        self.ask_peers().await;
                    }
                    self.elect();
                    self.list_due().await;
                }
                alive = self.world.changed() => {
                    if !alive {
                        return;
                    }
                    // A machine came or went: its reach decides the watcher.
                    let online = self.world.online();
                    if self.asked.as_ref().is_some_and(|(_, then)| *then != online) {
                        self.ask_peers().await;
                    }
                    for project in self.elect() {
                        self.list(&project).await;
                    }
                }
                Some(done) = self.waits.join_next_with_id(), if !self.waits.is_empty() => {
                    self.waited(done).await;
                }
            }
        }
    }

    /// Asks every online machine with a checkout of a pinned project how it
    /// reaches the hosts, all at once.
    async fn ask_peers(&mut self) {
        let me = self.world.me();
        let online = self.world.online();
        let asked: BTreeSet<EndpointId> = self
            .world
            .projects()
            .iter()
            .flat_map(|project| project.machines.iter().copied())
            .filter(|machine| *machine != me && online.contains(machine))
            .collect();
        self.asked = Some((Instant::now(), online));
        let world = &self.world;
        let answers = join_all(asked.into_iter().map(|peer| async move {
            let said = match timeout(ASK_LIMIT, world.ask(peer)).await {
                Ok(said) => said,
                Err(_) => Err(anyhow!("it did not answer in time")),
            };
            (peer, said)
        }))
        .await;
        self.peers = answers
            .into_iter()
            .filter_map(|(peer, said)| match said {
                Ok(reach) => Some((peer, reach)),
                Err(e) => {
                    log::debug!("CI watch: {peer} says nothing of its reach: {e:#}");
                    None
                }
            })
            .collect();
    }

    /// Elects the watcher of every pinned project; the projects this machine
    /// took on now. What it lost stops at once.
    fn elect(&mut self) -> Vec<Project> {
        let me = self.world.me();
        let online = self.world.online();
        self.projects = self.world.projects();
        let peers: BTreeMap<EndpointId, ForgeReach> = self
            .peers
            .iter()
            .filter(|(peer, _)| online.contains(*peer))
            .map(|(peer, reach)| (*peer, reach.clone()))
            .collect();
        let held: BTreeSet<String> = self
            .projects
            .iter()
            .filter(|project| {
                let reach = self.ci.reach_of(project.target.host_name());
                watcher_of(project, me, reach, &peers) == Some(me)
            })
            .map(|project| project.name.clone())
            .collect();
        let lost: Vec<String> = self.held.difference(&held).cloned().collect();
        for lost in lost {
            log::info!("CI watch: {lost} is watched elsewhere now");
            self.drop_waits(&lost);
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
    async fn check_due(&mut self) {
        let held: Vec<Project> = self
            .held_projects()
            .filter(|project| project.target.host == Host::GitHub)
            .collect();
        for project in held {
            let now = Instant::now();
            let checks = self.checks.entry(project.name.clone()).or_default();
            if !checks.due(now) {
                continue;
            }
            let answer = bounded(self.forge.check(&project.target, checks.etag())).await;
            let answer = match answer {
                Ok(Some(answer)) => Ok(answer),
                Ok(None) => continue,
                Err(e) => Err(e),
            };
            let checks = self.checks.entry(project.name.clone()).or_default();
            if checks.answered(answer, now, SystemTime::now()) {
                self.list(&project).await;
            }
        }
    }

    /// Lists every held project that is due: busy ones every `POLL`, the
    /// others every `QUIET`. A push seen here makes a project busy and due.
    async fn list_due(&mut self) {
        let now = Instant::now();
        let held: Vec<Project> = self.held_projects().collect();
        for project in held {
            if let Some(at) = self.world.pushed(&project).await {
                let before = self.pushes.insert(project.name.clone(), at);
                if before.is_some_and(|before| before != at) {
                    self.busy_until.insert(project.name.clone(), now + BUSY);
                    self.next.remove(&project.name);
                }
            }
            if self.next.get(&project.name).is_none_or(|next| now >= *next) {
                self.list(&project).await;
            }
        }
    }

    fn held_projects(&self) -> impl Iterator<Item = Project> + '_ {
        self.projects
            .iter()
            .filter(|project| self.held.contains(&project.name))
            .cloned()
    }

    /// One list of `project`'s runs: what ended is told, what goes is waited on.
    async fn list(&mut self, project: &Project) {
        let host = project.target.host_name().to_owned();
        let listed = async {
            let login = match self.logins.get(&host) {
                Some(login) => login.clone(),
                None => bounded(self.forge.login(&project.target)).await?,
            };
            let runs = bounded(self.forge.list(&project.target)).await?;
            Ok::<_, anyhow::Error>((login, runs))
        }
        .await;
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
        self.save().await;
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
            }
            stays
        });
        self.queued.retain(|key| key.0 != project);
        self.last.retain(|key, _| key.0 != project);
    }

    /// A wait ended: the run is read once, and told when it ended.
    async fn waited(&mut self, done: Result<(Id, Waited), JoinError>) {
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
        let Some(project) = self.held_projects().find(|p| p.name == waited.key.0) else {
            return;
        };
        let host = project.target.host_name().to_owned();
        let run = match bounded(self.forge.one(&project.target, waited.key.1)).await {
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
            self.tell(&project, run);
            self.save().await;
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
}

impl World for MeshWorld {
    fn me(&self) -> EndpointId {
        self.me
    }

    fn projects(&self) -> Vec<Project> {
        projects::pinned(&self.mesh)
    }

    fn online(&self) -> BTreeSet<EndpointId> {
        self.mesh
            .code_hosts()
            .into_iter()
            .filter(|status| status.online && status.device.id != self.me)
            .map(|status| status.device.id)
            .collect()
    }

    async fn ask(&self, peer: EndpointId) -> Result<ForgeReach> {
        let request = Request {
            session: String::new(),
            tool: Tool::ForgeReach,
            args: "{}".to_owned(),
            approved: Default::default(),
        };
        match over_the_mesh(&self.mesh, peer, &request).await? {
            Reply::Done {
                text,
                failed: false,
            } => Ok(serde_json::from_str(&text)?),
            Reply::Done { text, .. } => bail!("{text}"),
            _ => bail!("it answered something else"),
        }
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
