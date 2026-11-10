//! What each machine of the mesh reports over `status/1`, kept for a
//! redraw: who is on, what each one is, its load and traffic. One follow
//! per online member, started and stopped as the mesh changes, so a read
//! never waits on the network.

use beep_mesh::{Device, Mesh, PeerClient, SessionFn};
use beep_proto::{
    AgentHost, Machine, MachineApp, MachineCapability, MachineDetails, MachineDisk, MachineEnv,
    MachineGpu, MachineLogin, MachineStats, MachineTool, MachineTraffic, TrafficCounter,
    UpdatableMachine,
    node::{
        AppInfo, AppName, Capability, DiskUsage, EnvReport, GpuInfo, StatsReport,
        agent::agent_of,
        status::{PROTOCOL, Report},
        traffic::TrafficReport,
        update,
        version::{self, Version, clock_is_off, clock_offset_ms},
    },
};
use iroh::EndpointId;
use serde::Serialize;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::{
    select,
    sync::{oneshot, watch},
    task::{AbortHandle, JoinHandle, JoinSet},
    time::{Instant as TokioInstant, sleep_until},
};

/// How long to wait before following a member whose follow ended without the
/// mesh changing.
const RETRY: Duration = Duration::from_secs(15);

/// A machine whose clock is off, as the Machines screens read it.
#[derive(Debug, Serialize)]
pub struct ClockOff {
    id: String,
    /// How far ahead of this Mac's clock it is, in milliseconds; negative
    /// when behind.
    offset_ms: i64,
}

/// A report and when it came.
struct Sampled<T> {
    at: SystemTime,
    report: T,
}

/// One member of the mesh and the last it reported.
struct Peer {
    device: Device,
    online: bool,
    /// When it last reported, or its link went.
    seen_at: Option<SystemTime>,
    /// The beep-node it runs; not known to a node from before `version/1`.
    version: Option<String>,
    /// How far its clock is ahead of this Mac's, in milliseconds, when the
    /// node said its time in a round trip that was quick enough.
    clock_offset_ms: Option<i64>,
    offers: Vec<String>,
    env: Option<EnvReport>,
    stats: Option<Sampled<StatsReport>>,
    traffic: Option<Sampled<TrafficReport>>,
}

impl Peer {
    fn new(device: Device) -> Self {
        Self {
            device,
            online: false,
            seen_at: None,
            version: None,
            clock_offset_ms: None,
            offers: Vec::new(),
            env: None,
            stats: None,
            traffic: None,
        }
    }

    fn offers(&self, protocol: &str) -> bool {
        self.offers.iter().any(|offer| offer == protocol)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Revision {
    pub machines: u64,
    pub telemetry: u64,
}

/// The members and what they reported, shared with the follows.
struct Shared {
    peers: Mutex<BTreeMap<EndpointId, Peer>>,
    /// Counts every change, for a reader that redraws.
    changes: watch::Sender<Revision>,
}

impl Shared {
    fn peers(&self) -> MutexGuard<'_, BTreeMap<EndpointId, Peer>> {
        self.peers.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Takes the members and who is on from `mesh`, and drops those that
    /// are not members any more. One whose link just went was seen now.
    fn refresh(&self, mesh: &Mesh) {
        let devices = mesh.machines();
        let now = SystemTime::now();
        let mut peers = self.peers();
        let before = peers.len();
        peers.retain(|id, _| devices.iter().any(|status| status.device.id == *id));
        let mut changed = peers.len() != before;

        for status in devices {
            changed |= peers
                .get(&status.device.id)
                .is_none_or(|peer| peer.device != status.device || peer.online != status.online);
            let peer = peers
                .entry(status.device.id)
                .or_insert_with(|| Peer::new(status.device.clone()));
            if peer.online && !status.online {
                peer.seen_at = Some(now);
            }
            peer.device = status.device;
            peer.online = status.online;
        }

        drop(peers);
        if changed {
            self.changes
                .send_modify(|revision| revision.machines = revision.machines.wrapping_add(1));
        }
    }

    fn set_version(&self, id: EndpointId, version: String, clock_offset_ms: Option<i64>) {
        let mut peers = self.peers();
        let Some(peer) = peers.get_mut(&id) else {
            return;
        };
        let changed = peer.version.as_ref() != Some(&version);
        peer.version = Some(version);
        peer.clock_offset_ms = clock_offset_ms;
        drop(peers);
        if changed {
            self.changes
                .send_modify(|revision| revision.machines = revision.machines.wrapping_add(1));
        }
    }

    fn record(&self, id: EndpointId, report: Report) {
        let now = SystemTime::now();
        let mut peers = self.peers();
        let Some(peer) = peers.get_mut(&id) else {
            return;
        };
        peer.seen_at = Some(now);
        let (machines, telemetry) = match report {
            Report::Offers(offers) => {
                let changed = peer.offers != offers;
                peer.offers = offers;
                (changed, false)
            }
            Report::Env(env) => {
                let changed = peer.env.as_ref() != Some(&env);
                peer.env = Some(env);
                (changed, false)
            }
            Report::Stats(report) => {
                peer.stats = Some(Sampled { at: now, report });
                (false, true)
            }
            Report::Traffic(report) => {
                peer.traffic = Some(Sampled { at: now, report });
                (false, true)
            }
            _ => (false, false),
        };
        drop(peers);

        if machines || telemetry {
            self.changes.send_modify(|revision| {
                if machines {
                    revision.machines = revision.machines.wrapping_add(1);
                }
                if telemetry {
                    revision.telemetry = revision.telemetry.wrapping_add(1);
                }
            });
        }
    }
}

/// The task that keeps one follow per online member, and how to stop it.
struct Watching {
    stop: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

pub struct Status {
    me: EndpointId,
    /// The embedded node, which is followed in this process. None on a phone.
    session: Option<SessionFn>,
    shared: Arc<Shared>,
    watching: Mutex<Option<Watching>>,
}

impl Status {
    pub(super) fn new(me: EndpointId, session: Option<SessionFn>) -> Self {
        Self {
            me,
            session,
            shared: Arc::new(Shared {
                peers: Mutex::default(),
                changes: watch::channel(Revision::default()).0,
            }),
            watching: Mutex::new(None),
        }
    }

    /// Follows the members of `mesh` in place of those of the mesh before.
    /// The list of machines is read from it at once.
    pub(super) async fn watch(&self, mesh: Mesh) {
        self.stop().await;
        // What the machines reported stays for those that are members still.
        self.shared.refresh(&mesh);
        let (stop, stopped) = oneshot::channel();
        let follower = Follower {
            me: self.me,
            session: self.session.clone(),
            shared: self.shared.clone(),
            mesh,
        };
        let task = tokio::spawn(follower.run(stopped));
        *self.watching.lock().unwrap_or_else(PoisonError::into_inner) =
            Some(Watching { stop, task });
    }

    /// Stops every follow and waits until they let go of the mesh, so it
    /// can be closed.
    pub(super) async fn stop(&self) {
        let watching = self
            .watching
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(Watching { stop, task }) = watching {
            // A task that is gone already has nothing to stop.
            stop.send(()).ok();

            if let Err(e) = task.await {
                log::warn!("following the machines ended badly: {e}");
            }
        }
    }

    /// Counts up whenever a machine comes, goes or reports.
    pub(super) fn changes(&self) -> watch::Receiver<Revision> {
        self.shared.changes.subscribe()
    }

    /// The machines of the mesh, online or not. `version` is empty until the
    /// node says it, and for a node from before `version/1`; a machine that
    /// has not reported has no `env`.
    pub fn machines(&self) -> Vec<Machine> {
        self.shared
            .peers()
            .values()
            .map(|peer| Machine {
                id: peer.device.id.to_string(),
                name: peer.device.name.clone(),
                online: peer.online,
                seen_at: peer
                    .seen_at
                    .unwrap_or_else(|| UNIX_EPOCH + Duration::from_millis(peer.device.added_at_ms)),
                version: peer.version.clone().unwrap_or_default(),
                env: peer.env.clone().map(machine_env),
            })
            .collect()
    }

    /// The beep-node `machine` runs, once it said.
    pub fn version(&self, machine: EndpointId) -> Option<String> {
        self.shared.peers().get(&machine)?.version.clone()
    }

    /// What `machine` last reported about itself, if it did.
    pub fn last_env(&self, machine: EndpointId) -> Option<EnvReport> {
        self.shared.peers().get(&machine)?.env.clone()
    }

    /// The apps, graphics cards and abilities of each machine; one that has
    /// not reported is listed empty.
    pub fn details(&self) -> Vec<MachineDetails> {
        self.shared
            .peers()
            .values()
            .map(|peer| machine_details(peer.device.id.to_string(), peer.env.clone()))
            .collect()
    }

    /// The live resources of the machines that are on and sent them.
    pub fn stats(&self) -> Vec<MachineStats> {
        self.shared
            .peers()
            .values()
            .filter(|peer| peer.online)
            .filter_map(|peer| {
                let Sampled { at, report } = peer.stats.as_ref()?;
                Some(MachineStats {
                    id: peer.device.id.to_string(),
                    sampled_at: *at,
                    disks: report.disks.iter().cloned().map(machine_disk).collect(),
                    memory_used_mb: report.memory_used_mb,
                    memory_total_mb: report.memory_total_mb,
                    load: report.load,
                })
            })
            .collect()
    }

    /// The machines that are on and whose clock is far enough off to warn
    /// about, with how far ahead of this Mac's it is (negative: behind).
    pub fn clocks_off(&self) -> Vec<ClockOff> {
        self.shared
            .peers()
            .values()
            .filter(|peer| peer.online)
            .filter_map(|peer| {
                let offset = peer
                    .clock_offset_ms
                    .filter(|offset| clock_is_off(*offset))?;
                Some(ClockOff {
                    id: peer.device.id.to_string(),
                    offset_ms: offset,
                })
            })
            .collect()
    }

    /// What each machine that reported moved over iroh.
    pub fn traffic(&self) -> Vec<MachineTraffic> {
        self.shared
            .peers()
            .values()
            .filter_map(|peer| {
                let Sampled { at, report } = peer.traffic.as_ref()?;
                Some(MachineTraffic {
                    id: peer.device.id.to_string(),
                    updated_at: *at,
                    counters: report
                        .counters
                        .iter()
                        .map(|(key, bytes)| TrafficCounter {
                            key: key.clone(),
                            bytes: *bytes,
                        })
                        .collect(),
                })
            })
            .collect()
    }

    /// The machines that are on and update over `update/1`. Their agent
    /// sessions are not counted: the update itself asks.
    pub fn updatable(&self) -> Vec<UpdatableMachine> {
        self.shared
            .peers()
            .values()
            .filter(|peer| peer.online && peer.offers(update::PROTOCOL))
            .map(|peer| UpdatableMachine {
                id: peer.device.id.to_string(),
                agent_sessions: 0,
            })
            .collect()
    }

    /// The machines that are on and offer a coding agent that `is_off` does
    /// not switch off. A status report carries no workspace.
    pub fn agent_hosts(&self, is_off: impl Fn(&str) -> bool) -> Vec<AgentHost> {
        self.shared
            .peers()
            .values()
            .filter(|peer| peer.online)
            .filter_map(|peer| {
                let agents: Vec<String> = peer
                    .offers
                    .iter()
                    .filter_map(|offer| agent_of(offer))
                    .filter(|agent| !is_off(agent))
                    .map(str::to_owned)
                    .collect();
                if agents.is_empty() {
                    return None;
                }

                let env = peer.env.as_ref();
                Some(AgentHost {
                    machine_id: peer.device.id.to_string(),
                    machine_name: peer.device.name.clone(),
                    agents,
                    workspace: None,
                    home: env
                        .map(|env| env.home.clone())
                        .filter(|home| !home.is_empty()),
                    capabilities: env
                        .iter()
                        .flat_map(|env| env.capabilities())
                        .map(machine_capability)
                        .collect(),
                })
            })
            .collect()
    }
}

/// What keeps the follows of one mesh going.
struct Follower {
    me: EndpointId,
    session: Option<SessionFn>,
    shared: Arc<Shared>,
    mesh: Mesh,
}

impl Follower {
    /// Keeps one follow per online member until `stopped`, and lets go of
    /// the mesh once every follow has ended.
    async fn run(self, mut stopped: oneshot::Receiver<()>) {
        let mut updates = self.mesh.updates();
        let mut follows = JoinSet::new();
        let mut running: BTreeMap<EndpointId, AbortHandle> = BTreeMap::new();
        // When each follow that ended last did, so it is not started again
        // on every change of the mesh.
        let mut ended: BTreeMap<EndpointId, Instant> = BTreeMap::new();

        loop {
            updates.borrow_and_update();
            self.shared.refresh(&self.mesh);
            reap(&mut follows);
            running.retain(|id, follow| {
                let finished = follow.is_finished();
                if finished {
                    ended.insert(*id, Instant::now());
                }
                !finished
            });
            ended.retain(|id, _| self.shared.peers().contains_key(id));
            // The soonest a follow that ended to an online machine is tried again.
            let mut retry: Option<Instant> = None;

            for status in self.mesh.code_hosts().into_iter().filter(|s| s.online) {
                let id = status.device.id;

                if running.contains_key(&id) {
                    continue;
                }

                if let Some(at) = ended.get(&id).copied()
                    && !retry_due(Some(at))
                {
                    let next = at + RETRY;
                    retry = Some(retry.map_or(next, |soonest| soonest.min(next)));
                    continue;
                }

                let client = match (&self.session, id == self.me) {
                    (Some(session), true) => PeerClient::local(session.clone()),
                    // A phone is no machine of the mesh, so it never follows itself.
                    (None, true) => continue,
                    (_, false) => PeerClient::over(self.mesh.clone(), id),
                };
                let follow = follows.spawn(follow(client, id, self.shared.clone()));
                running.insert(id, follow);
            }

            select! {
                changed = updates.changed() => {
                    if changed.is_err() {
                        break;
                    }
                }
                // A follow ended: it is tried again after `RETRY`.
                Some(_) = follows.join_next() => {}
                // Only while a follow waits to start again; an offline
                // machine coming back is a mesh update.
                () = sleep_until(retry.map_or_else(TokioInstant::now, TokioInstant::from_std)), if retry.is_some() => {}
                // Sent, or the status is gone.
                _ = &mut stopped => break,
            }
        }

        follows.shutdown().await;
    }
}

/// Collects the follows that have ended, so the set holds only live ones.
fn reap(follows: &mut JoinSet<()>) {
    while follows.try_join_next().is_some() {}
}

/// Whether a follow that ended at `ended`, if one did, may start again.
fn retry_due(ended: Option<Instant>) -> bool {
    ended.is_none_or(|at| at.elapsed() >= RETRY)
}

/// This Mac's Unix time in milliseconds.
fn unix_ms() -> Option<u64> {
    let since = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
    u64::try_from(since.as_millis()).ok()
}

/// The beep-node the machine runs, when it says, and how far its clock is
/// ahead of ours, measured over the same round trip.
async fn node_version(client: &PeerClient) -> Option<(String, Option<i64>)> {
    let before = unix_ms();
    let (_tx, mut rx) = client.open(version::PROTOCOL).await.ok()?;
    let Version { version, now_ms } = rx.recv().await.ok()??;
    let offset = match (before, unix_ms(), now_ms) {
        (Some(before), Some(after), Some(remote)) => clock_offset_ms(before, after, remote),
        _ => None,
    };
    Some((version, offset))
}

/// Takes the reports of one machine until it closes the stream or fails.
async fn follow(client: PeerClient, id: EndpointId, shared: Arc<Shared>) {
    let mut startup = JoinSet::new();
    startup.spawn({
        let client = client.clone();
        async move { client.prepare_events().await }
    });
    startup.spawn({
        let client = client.clone();
        let shared = shared.clone();
        async move {
            if let Some((version, offset)) = node_version(&client).await {
                shared.set_version(id, version, offset);
            }
        }
    });

    // Our half stays open: finishing it tells the node we are done.
    let (_tx, mut rx) = match client.open(PROTOCOL).await {
        Ok(stream) => stream,
        Err(e) => {
            log::debug!("no status from {}: {e}", id.fmt_short());
            return;
        }
    };

    loop {
        select! {
            report = rx.recv::<Report>() => match report {
                Ok(Some(report)) => shared.record(id, report),
                Ok(None) => return,
                Err(e) => {
                    log::debug!("status from {} broke: {e:#}", id.fmt_short());
                    return;
                }
            },
            Some(result) = startup.join_next(), if !startup.is_empty() => {
                if let Err(error) = result {
                    log::debug!("startup probe for {} stopped: {error}", id.fmt_short());
                }
            }
        }
    }
}

fn machine_env(env: EnvReport) -> MachineEnv {
    MachineEnv {
        os: env.os,
        os_version: env.os_version,
        arch: env.arch,
        hostname: env.hostname,
        cpus: env.cpus,
        memory_mb: env.memory_mb,
        home: env.home,
        tools: env
            .tools
            .into_iter()
            .map(|tool| MachineTool {
                name: tool.name,
                version: tool.version,
            })
            .collect(),
        logins: env
            .logins
            .into_iter()
            .map(|login| MachineLogin {
                tool: login.tool,
                signed_in: login.signed_in,
                account: login.account,
            })
            .collect(),
        avds: env.avds,
    }
}

fn machine_details(id: String, env: Option<EnvReport>) -> MachineDetails {
    let env = env.unwrap_or_default();
    MachineDetails {
        id,
        capabilities: env
            .capabilities()
            .into_iter()
            .map(machine_capability)
            .collect(),
        apps: env
            .apps
            .into_iter()
            .filter(|app| app.name != AppName::Unknown)
            .map(machine_app)
            .collect(),
        gpus: env.gpus.into_iter().map(machine_gpu).collect(),
    }
}

/// "Unreal Engine 5.4" for the engine, whose versions are part of its name.
fn machine_app(app: AppInfo) -> MachineApp {
    match (app.name, &app.version) {
        (AppName::UnrealEngine, Some(version)) => MachineApp {
            name: format!("{} {version}", app.name),
            version: app.version,
        },
        _ => MachineApp {
            name: app.name.to_string(),
            version: app.version,
        },
    }
}

fn machine_gpu(gpu: GpuInfo) -> MachineGpu {
    MachineGpu {
        vendor: gpu.vendor.to_string(),
        model: gpu.model,
        vram_mb: gpu.vram_mb,
        driver: gpu.driver,
    }
}

fn machine_capability(capability: Capability) -> MachineCapability {
    match capability {
        Capability::Gpu => MachineCapability::Gpu {},
        Capability::Cuda => MachineCapability::Cuda {},
        Capability::Blender => MachineCapability::Blender {},
        Capability::Unreal => MachineCapability::Unreal {},
        Capability::Xcode => MachineCapability::Xcode {},
        Capability::Android => MachineCapability::Android {},
        Capability::Docker => MachineCapability::Docker {},
    }
}

fn machine_disk(disk: DiskUsage) -> MachineDisk {
    MachineDisk {
        path: disk.path,
        free_bytes: disk.free_bytes,
        total_bytes: disk.total_bytes,
    }
}

#[cfg(test)]
#[path = "../tests/fleet_status.rs"]
mod tests;
