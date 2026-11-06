//! The task that keeps this machine's skills in step with the mesh, and the
//! `skills/1` answers other nodes read.

use super::{
    Skills,
    compare::{Holder, Outcome, compare},
    disk::{self, Scan, Seen},
};
use anyhow::{Context, Result, bail};
use beep_mesh::{Mesh, MeshError, PeerClient};
use beep_proto::node::{
    LinkRx, LinkTx,
    skills::{Conflict, Listed, PROTOCOL, Reply, Request, SkillFile},
};
use iroh::EndpointId;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    mem,
    path::PathBuf,
    time::Duration,
};
use tokio::{
    task::spawn_blocking,
    time::{MissedTickBehavior, interval, timeout},
};

/// How often the machines are compared when nothing comes or goes.
const EVERY: Duration = Duration::from_secs(120);
/// The most skills taken in one pass; the rest wait for the next.
const MAX_TAKEN: usize = 32;
/// How long a peer has to send its request.
const READ_LIMIT: Duration = Duration::from_secs(10);
/// The record of the names this machine held, in the node's folder.
const SEEN: &str = "skills-seen";

/// Compares this machine's skills with every other machine's now, when a
/// machine comes or goes, and every `EVERY`, until the mesh stops. Nothing
/// when the host does not sync skills.
pub(crate) async fn follow(mesh: Mesh, dir: PathBuf, skills: Skills) {
    let Some(home) = skills.home().cloned() else {
        return;
    };
    let file = dir.join(SEEN);
    let seen = match spawn_blocking(move || Seen::load(&file)).await {
        Ok(Ok(seen)) => seen,
        Ok(Err(e)) => {
            log::warn!("skills do not sync: {e:#}");
            return;
        }
        Err(e) => {
            log::warn!("skills do not sync: {e}");
            return;
        }
    };
    let mut pass = Passes {
        mesh: mesh.clone(),
        home,
        seen,
        lists: HashMap::new(),
        told: BTreeMap::new(),
        said: None,
        skills,
    };
    let mut updates = mesh.updates();
    let mut tick = interval(EVERY);
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
    tick.tick().await;
    loop {
        let now = online(&mesh);
        pass.run(&now).await;
        loop {
            tokio::select! {
                _ = tick.tick() => break,
                changed = updates.changed() => {
                    if changed.is_err() {
                        return;
                    }
                    if online(&mesh) != now {
                        break;
                    }
                }
            }
        }
    }
}

/// The other members online now.
fn online(mesh: &Mesh) -> BTreeSet<EndpointId> {
    let me = mesh.me();
    mesh.devices()
        .into_iter()
        .filter(|status| status.online && status.device.id != me)
        .map(|status| status.device.id)
        .collect()
}

/// What one pass keeps for the next, owned by the `follow` task alone.
struct Passes {
    mesh: Mesh,
    home: PathBuf,
    seen: Seen,
    /// Each other machine's last list, kept while it is offline so its
    /// conflicts stay told.
    lists: HashMap<EndpointId, Vec<Listed>>,
    /// The conflicts last logged, by name.
    told: BTreeMap<String, Conflict>,
    /// The summary last logged.
    said: Option<Summary>,
    skills: Skills,
}

/// What a pass found, logged when it changes.
#[derive(Clone, Copy, PartialEq)]
struct Summary {
    /// Skill names here.
    held: usize,
    /// Other machines whose list came.
    compared: usize,
    /// Names that differ.
    differ: usize,
}

impl Passes {
    async fn run(&mut self, online: &BTreeSet<EndpointId>) {
        let Some(scan) = self.scan().await else {
            return;
        };
        let mut compared = 0;
        for &peer in online {
            let client = PeerClient::over(self.mesh.clone(), peer);
            match client.call(PROTOCOL, &Request::List {}).await {
                Ok(Reply::Skills { skills }) => {
                    self.lists.insert(peer, skills);
                    compared += 1;
                }
                Ok(other) => log::debug!("skills: {peer} answered {other:?}"),
                // A node from before skills/1, or one that does not sync them.
                Err(MeshError::Declined { .. }) => {
                    self.lists.remove(&peer);
                }
                Err(e) => log::debug!("skills: {peer} not read: {e}"),
            }
        }
        let mesh = &self.mesh;
        self.lists.retain(|id, _| mesh.is_member(*id));
        let names: HashMap<EndpointId, String> = mesh
            .devices()
            .into_iter()
            .map(|status| (status.device.id, status.device.name))
            .collect();
        let name_of = |id: &EndpointId| {
            names
                .get(id)
                .cloned()
                .unwrap_or_else(|| id.fmt_short().to_string())
        };
        let here = name_of(&mesh.me());
        let mut machines: Vec<(EndpointId, String)> =
            self.lists.keys().map(|id| (*id, name_of(id))).collect();
        machines.sort_by(|a, b| a.1.cmp(&b.1));
        let mut holders = vec![Holder {
            machine: &here,
            listed: &scan.listed,
        }];
        for (id, name) in &machines {
            if let Some(listed) = self.lists.get(id) {
                holders.push(Holder {
                    machine: name,
                    listed,
                });
            }
        }
        let Outcome { conflicts, wanted } = compare(&holders, |name| {
            !scan.names.contains(name) && !self.seen.contains(name)
        });
        drop(holders);
        for want in wanted.into_iter().take(MAX_TAKEN) {
            let from = self
                .lists
                .iter()
                .filter(|(id, _)| online.contains(*id))
                .find(|(_, listed)| {
                    listed
                        .iter()
                        .any(|l| l.name == want.name && l.hash == want.hash)
                })
                .map(|(id, _)| *id);
            let Some(from) = from else {
                continue;
            };
            let request = Request::Get {
                name: want.name.clone(),
                hash: want.hash.clone(),
            };
            let files = match PeerClient::over(self.mesh.clone(), from)
                .call(PROTOCOL, &request)
                .await
            {
                Ok(Reply::Files { files }) => files,
                Ok(_) => continue,
                Err(e) => {
                    log::debug!("skills: {} not fetched: {e}", want.name);
                    continue;
                }
            };
            if files.is_empty() || disk::hash(&files) != want.hash {
                // Changed since it was listed: the next pass sees it anew.
                continue;
            }
            match self.take(&want.name, files).await {
                Ok(true) => log::info!("skills: got {} from {}", want.name, name_of(&from)),
                Ok(false) => {}
                Err(e) => log::warn!("skill {} not written: {e:#}", want.name),
            }
        }
        self.tell(conflicts);
        let summary = Summary {
            held: scan
                .listed
                .iter()
                .map(|l| &l.name)
                .collect::<BTreeSet<_>>()
                .len(),
            compared,
            differ: self.told.len(),
        };
        if self.said != Some(summary) {
            log::info!(
                "skills: {} here, {} other machines compared, {} differ",
                summary.held,
                summary.compared,
                summary.differ
            );
            self.said = Some(summary);
        }
    }

    /// The skills on disk now, each name among them kept as held.
    async fn scan(&mut self) -> Option<Scan> {
        let home = self.home.clone();
        let mut seen = mem::take(&mut self.seen);
        let done = spawn_blocking(move || {
            let scan = disk::scan(&home);
            let kept = seen.add(scan.listed.iter().map(|l| l.name.as_str()));
            (seen, scan, kept)
        })
        .await;
        match done {
            Ok((seen, scan, kept)) => {
                self.seen = seen;
                if let Err(e) = kept {
                    log::warn!("skills: the record of held names not written: {e:#}");
                }
                Some(scan)
            }
            Err(e) => {
                log::warn!("skills: reading the skills folders failed: {e}");
                None
            }
        }
    }

    /// Writes a skill from another machine and keeps its name as held.
    async fn take(&mut self, name: &str, files: Vec<SkillFile>) -> Result<bool> {
        let home = self.home.clone();
        let name = name.to_owned();
        let mut seen = mem::take(&mut self.seen);
        let (seen, wrote) = spawn_blocking(move || {
            let wrote = disk::install(&home, &name, &files)
                .and_then(|wrote| seen.add([name.as_str()]).map(|()| wrote));
            (seen, wrote)
        })
        .await
        .context("writing the skill stopped")?;
        self.seen = seen;
        wrote
    }

    /// Logs each conflict that is new or changed, and hands them to the
    /// desktop.
    fn tell(&mut self, conflicts: Vec<Conflict>) {
        let now: BTreeMap<String, Conflict> = conflicts
            .into_iter()
            .map(|conflict| (conflict.name.clone(), conflict))
            .collect();
        for (name, conflict) in &now {
            if self.told.get(name) != Some(conflict) {
                log::warn!(
                    "{}; nothing changed ({})",
                    conflict.title(),
                    conflict.copies.join(", ")
                );
            }
        }
        self.told = now;
        let list: Vec<Conflict> = self.told.values().cloned().collect();
        self.skills.conflicts.send_if_modified(|before| {
            let changed = *before != list;
            if changed {
                *before = list;
            }
            changed
        });
    }
}

/// Answers one `skills/1` request.
pub(crate) async fn serve(mut tx: LinkTx, mut rx: LinkRx, skills: Skills) -> Result<()> {
    let request: Request = timeout(READ_LIMIT, rx.recv())
        .await
        .context("the peer sent no request")??
        .context("the stream closed before its request")?;
    let Some(home) = skills.home().cloned() else {
        bail!("this node does not sync skills");
    };
    let reply = match request {
        Request::Conflicts {} => Reply::Conflicts {
            conflicts: skills.conflicts.borrow().clone(),
        },
        Request::List {} => Reply::Skills {
            skills: blocking(move || disk::scan(&home).listed).await?,
        },
        Request::Get { name, hash } => Reply::Files {
            files: blocking(move || disk::files_of(&home, &name, &hash)).await?,
        },
    };
    tx.send(&reply).await?;
    tx.finish().await
}

async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Result<T> {
    spawn_blocking(work)
        .await
        .context("reading the skills folders stopped")
}
