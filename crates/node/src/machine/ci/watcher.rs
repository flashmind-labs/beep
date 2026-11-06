//! The watch itself: one task per node that owns everything it knows. Each
//! minute it asks the other machines how they reach the hosts, elects the
//! watcher of every pinned project and lists the runs of those it holds.
//! A mesh change elects again. Going runs are waited on by at most
//! `MAX_WAITS` CLI processes at once, owned here and killed when the role
//! moves away or the node stops.

use super::{
    Ci,
    book::Books,
    elect::elect,
    forge::{Cli, Forge, Target},
    projects::{self, Project},
};
use crate::agents::mcp::over_the_mesh;
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
    future::Future,
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime},
};
use tokio::{
    select,
    sync::watch,
    task::{AbortHandle, Id, JoinError, JoinSet},
    time::{MissedTickBehavior, interval, timeout},
};

/// How often the runs are listed, and the machines asked.
pub(super) const POLL: Duration = Duration::from_secs(60);
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
}

/// Watches while the node is in `mesh`, keeping its file in `dir`.
pub(crate) async fn watch(mesh: Mesh, me: EndpointId, dir: PathBuf, ci: Arc<Ci>) {
    let world = MeshWorld {
        updates: mesh.updates(),
        mesh,
        me,
    };
    Watcher::start(Cli, world, ci, dir.join(FILE))
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
}

impl<F: Forge, W: World> Watcher<F, W> {
    pub(super) async fn start(forge: F, world: W, ci: Arc<Ci>, file: PathBuf) -> Self {
        Self {
            forge: Arc::new(forge),
            world,
            ci,
            books: Books::load(&file).await,
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
        }
    }

    pub(super) async fn run(mut self) {
        let mut tick = interval(POLL);
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            select! {
                _ = tick.tick() => {
                    self.ask_peers().await;
                    self.elect();
                    self.list_due().await;
                }
                alive = self.world.changed() => {
                    if !alive {
                        return;
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
        gained
    }

    /// Lists every held project.
    async fn list_due(&mut self) {
        let due: Vec<Project> = self.held_projects().collect();
        for project in due {
            self.list(&project).await;
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

    async fn save(&self) {
        if let Err(e) = self.books.save(&self.file).await {
            log::warn!("CI watch: {e:#}");
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
            .devices()
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
}
