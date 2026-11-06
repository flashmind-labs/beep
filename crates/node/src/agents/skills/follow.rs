//! The task that keeps this machine's skills in step with the mesh, and the
//! `skills/1` answers other nodes read.

use super::{
    Skills,
    compare::{Holder, Outcome, Take, compare},
    disk::{self, Base, Scan, Seen},
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
/// machine comes or goes or says its skills changed, and every `EVERY`,
/// until the mesh stops. Nothing when the host does not sync skills.
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
    let nudge = skills.nudge.clone();
    let mut pass = Passes {
        mesh: mesh.clone(),
        home,
        seen,
        lists: HashMap::new(),
        sent: HashMap::new(),
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
        // A pass that took a skill runs again, so the others hear of it.
        if pass.run(&now).await {
            continue;
        }
        loop {
            tokio::select! {
                _ = tick.tick() => break,
                () = nudge.notified() => break,
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
    /// The list each machine was last told changed.
    sent: HashMap<EndpointId, Vec<Listed>>,
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
    /// One pass; whether it wrote a skill.
    async fn run(&mut self, online: &BTreeSet<EndpointId>) -> bool {
        let Some(scan) = self.scan().await else {
            return false;
        };
        let here = self.listing(&scan);
        self.skills.listed.send_replace(Some(here.clone()));
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
        let mesh = self.mesh.clone();
        self.lists.retain(|id, _| mesh.is_member(*id));
        // A pass that kept a copy runs again, listing it as picked.
        if self.keep(&here).await {
            self.save().await;
            return true;
        }
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
        let me = name_of(&mesh.me());
        let mut machines: Vec<(EndpointId, String)> =
            self.lists.keys().map(|id| (*id, name_of(id))).collect();
        machines.sort_by(|a, b| a.1.cmp(&b.1));
        let mut holders = vec![Holder {
            id: mesh.me(),
            machine: &me,
            here: true,
            listed: &here,
        }];
        for (id, name) in &machines {
            if let Some(listed) = self.lists.get(id) {
                holders.push(Holder {
                    id: *id,
                    machine: name,
                    here: false,
                    listed,
                });
            }
        }
        let Outcome {
            conflicts,
            takes,
            synced,
        } = compare(&holders, |name| {
            !scan.names.contains(name) && !self.seen.contains(name)
        });
        drop(holders);
        let mut wrote = false;
        for take in takes.into_iter().take(MAX_TAKEN) {
            let Some((files, from)) = self.files_for(&take, &here, online, &me).await else {
                continue;
            };
            let fresh = take.old.is_empty();
            match self.write(&take, files).await {
                Ok(true) => {
                    wrote = true;
                    self.seen.set(&take.name, take.base.clone());
                    let from =
                        from.map_or_else(|| "another folder here".to_owned(), |id| name_of(&id));
                    if fresh {
                        log::info!("skills: got {} from {from}", take.name);
                    } else {
                        log::info!("skills: updated {} from {from}", take.name);
                    }
                }
                Ok(false) => {}
                Err(e) => log::warn!("skill {} not written: {e:#}", take.name),
            }
        }
        for record in synced {
            self.seen.set(&record.name, record.base);
        }
        self.tell(conflicts);
        self.save().await;
        if !wrote {
            self.say_changed(online, &here).await;
        }
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
        wrote
    }

    /// The skills on disk now.
    async fn scan(&self) -> Option<Scan> {
        let home = self.home.clone();
        match spawn_blocking(move || disk::scan(&home)).await {
            Ok(scan) => Some(scan),
            Err(e) => {
                log::warn!("skills: reading the skills folders failed: {e}");
                None
            }
        }
    }

    /// `scan`'s copies with what this machine last synced of each: its
    /// version, and whether the copy was edited since. A name held for the
    /// first time with one content counts as synced as it is.
    fn listing(&mut self, scan: &Scan) -> Vec<Listed> {
        let mut contents: BTreeMap<&str, usize> = BTreeMap::new();
        for listed in &scan.listed {
            *contents.entry(&listed.name).or_default() += 1;
        }
        for (name, count) in contents {
            self.seen.hold(name);
            if self.seen.base(name).is_none()
                && count == 1
                && let Some(listed) = scan.listed.iter().find(|l| l.name == name)
            {
                self.seen.set(
                    name,
                    Base {
                        hash: listed.hash.clone(),
                        ver: 0,
                        kept: false,
                    },
                );
            }
        }
        scan.listed
            .iter()
            .map(|listed| {
                let base = self.seen.base(&listed.name);
                let synced = base.is_some_and(|base| base.hash == listed.hash);
                Listed {
                    ver: base.map_or(0, |base| base.ver),
                    edited: base.is_some() && !synced,
                    kept: synced && base.is_some_and(|base| base.kept),
                    ..listed.clone()
                }
            })
            .collect()
    }

    /// The files of `take`'s copy: from a folder here that holds it, or
    /// from a machine online that does (`Some` of it). None when no copy
    /// came whole.
    async fn files_for(
        &self,
        take: &Take,
        here: &[Listed],
        online: &BTreeSet<EndpointId>,
        me: &str,
    ) -> Option<(Vec<SkillFile>, Option<EndpointId>)> {
        let hash = &take.base.hash;
        let files = if here.iter().any(|l| l.name == take.name && &l.hash == hash) {
            let home = self.home.clone();
            let name = take.name.clone();
            let wanted = hash.clone();
            let files = spawn_blocking(move || disk::files_of(&home, &name, &wanted))
                .await
                .ok()?;
            (files, None)
        } else {
            let from = self
                .lists
                .iter()
                .filter(|(id, _)| online.contains(*id))
                .find(|(_, listed)| {
                    listed
                        .iter()
                        .any(|l| l.name == take.name && &l.hash == hash)
                })
                .map(|(id, _)| *id)?;
            let request = Request::Get {
                name: take.name.clone(),
                hash: hash.clone(),
            };
            match PeerClient::over(self.mesh.clone(), from)
                .call(PROTOCOL, &request)
                .await
            {
                Ok(Reply::Files { files }) => (files, Some(from)),
                Ok(_) => return None,
                Err(e) => {
                    log::debug!("skills: {} not fetched on {me}: {e}", take.name);
                    return None;
                }
            }
        };
        // Changed since it was listed: the next pass sees it anew.
        (!files.0.is_empty() && disk::hash(&files.0) == *hash).then_some(files)
    }

    /// Writes `take`'s files: a new skill into the agents' folders, or in
    /// place of the old copies here that are still as listed.
    async fn write(&self, take: &Take, files: Vec<SkillFile>) -> Result<bool> {
        let home = self.home.clone();
        let name = take.name.clone();
        let old = take.old.clone();
        spawn_blocking(move || {
            if old.is_empty() {
                return disk::install(&home, &name, &files);
            }
            let mut replaced = 0;
            for copy in &old {
                replaced += disk::replace(&home, &name, &files, &copy.folders, &copy.hash)?;
            }
            Ok(replaced > 0)
        })
        .await
        .context("writing the skill stopped")?
    }

    /// Applies the copies the user kept here: each becomes a picked version
    /// one past every version listed in the mesh and replaces the other
    /// copies here; the other machines take it in their next pass. Whether
    /// any was kept.
    async fn keep(&mut self, here: &[Listed]) -> bool {
        let picks = self.skills.picks.send_replace(BTreeMap::new());
        let mut kept = false;
        for (name, hash) in picks {
            let ver = here
                .iter()
                .chain(self.lists.values().flatten())
                .filter(|listed| listed.name == name)
                .map(|listed| listed.ver.saturating_add(u64::from(listed.edited)))
                .max()
                .unwrap_or(0)
                .saturating_add(1);
            let old: Vec<Listed> = here
                .iter()
                .filter(|listed| listed.name == name && listed.hash != hash)
                .cloned()
                .collect();
            let home = self.home.clone();
            let (key, wanted) = (name.clone(), hash.clone());
            match spawn_blocking(move || disk::keep(&home, &key, &wanted, &old)).await {
                Ok(Ok(())) => {
                    log::info!("skills: kept this machine's {name}; the other copies take it");
                    self.seen.set(
                        &name,
                        Base {
                            hash,
                            ver,
                            kept: true,
                        },
                    );
                    kept = true;
                }
                Ok(Err(e)) => log::warn!("skill {name} not kept: {e:#}"),
                Err(e) => log::warn!("keeping skill {name} stopped: {e}"),
            }
        }
        kept
    }

    /// Writes the record of held names when it changed.
    async fn save(&mut self) {
        let Some(saved) = self.seen.unsaved() else {
            return;
        };
        match spawn_blocking(move || saved.write()).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => log::warn!("skills: the record of held names not written: {e:#}"),
            Err(e) => log::warn!("skills: writing the record of held names stopped: {e}"),
        }
    }

    /// Tells each machine online whose last word from here is older than
    /// `here` that this machine's skills changed, so it compares again.
    async fn say_changed(&mut self, online: &BTreeSet<EndpointId>, here: &[Listed]) {
        for &peer in online {
            if self.sent.get(&peer).is_some_and(|sent| sent == here) {
                continue;
            }
            let client = PeerClient::over(self.mesh.clone(), peer);
            match client
                .call::<_, Reply>(PROTOCOL, &Request::Changed {})
                .await
            {
                Ok(_) => {
                    self.sent.insert(peer, here.to_vec());
                }
                Err(e) => log::debug!("skills: {peer} not told: {e}"),
            }
        }
        self.sent.retain(|id, _| online.contains(id));
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

/// Answers one `skills/1` request; `desktop` says a desktop asks.
pub(crate) async fn serve(
    mut tx: LinkTx,
    mut rx: LinkRx,
    skills: Skills,
    desktop: bool,
) -> Result<()> {
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
        Request::List {} => {
            // The first pass lists them; it runs as the node joins.
            let mut listed = skills.listed.subscribe();
            let skills = timeout(READ_LIMIT, listed.wait_for(Option::is_some))
                .await
                .context("the skills were not listed in time")?
                .context("the node stopped")?
                .clone()
                .unwrap_or_default();
            Reply::Skills { skills }
        }
        Request::Get { name, hash } => Reply::Files {
            files: spawn_blocking(move || disk::files_of(&home, &name, &hash))
                .await
                .context("reading the skills folders stopped")?,
        },
        Request::Changed {} => {
            skills.nudge.notify_one();
            Reply::Skills { skills: Vec::new() }
        }
        Request::Keep { name, hash } => {
            if !desktop {
                bail!("only a desktop keeps a copy of a skill");
            }
            let held = skills
                .listed
                .borrow()
                .as_ref()
                .is_some_and(|listed| listed.iter().any(|l| l.name == name && l.hash == hash));
            if !held {
                bail!("this machine has no such copy of {name}");
            }
            skills.picks.send_modify(|picks| {
                picks.insert(name, hash);
            });
            skills.nudge.notify_one();
            Reply::Skills { skills: Vec::new() }
        }
    };
    tx.send(&reply).await?;
    tx.finish().await
}
