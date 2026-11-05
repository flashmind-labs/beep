//! Background copies: every so often this node asks each machine of the mesh
//! for the sessions it offers (those of the mesh's projects, active in the
//! last 30 days) and keeps a copy of each in the cache, brought up to date
//! with only the bytes after what it holds. A copy starts, stops and resumes
//! nothing: it waits in the cache until the user continues it.
//!
//! Bounds: one copy at a time, a bytes-a-second rate, a cap on the cache's
//! size, a pause while a session runs here, and at most `MAX_FETCHES`
//! copies brought up to date per pass.

use super::{
    Env,
    cache::{Entry, Meta, evictions},
    dial::dial,
    offers::{Project, harness_of, is_recent, now_secs, projects_of},
    read::{Ask, Kind, Offer, Reply, Request, pull},
};
use anyhow::{Context, Result, bail};
use beep_mesh::Key;
use beep_proto::node::mesh_state::{SessionMove, StateValue};
use beep_proto::{
    AgentSlug,
    node::{
        agent::{CopyView, Status},
        link,
    },
};
use iroh::EndpointId;
use std::{
    cmp::Ordering,
    collections::HashMap,
    fs::{self, File},
    io::Read,
    path::Path,
    path::PathBuf,
    sync::Arc,
    time::Duration,
};
use tokio::{task::spawn_blocking, time::sleep};

/// How often a pass runs.
const EVERY: Duration = Duration::from_secs(60);
/// The most the cache holds.
const CAP: u64 = 2 * 1024 * 1024 * 1024;
/// How fast a copy is received, bytes a second.
const RATE: u64 = 8 * 1024 * 1024;
/// What the note beside a copy is allowed to take.
const NOTE_ROOM: u64 = 1024;
/// How long a copy with no note, which no pull went on with, is kept.
const PARTIAL_FOR: Duration = Duration::from_secs(24 * 3600);
/// The most copies brought up to date in one pass.
const MAX_FETCHES: usize = 16;

/// What bounds the copying; the defaults unless the environment says.
#[derive(Debug, Clone, Copy)]
pub(super) struct Limits {
    pub(super) every: Duration,
    pub(super) cap: u64,
    pub(super) rate: u64,
}

impl Limits {
    /// `BEEP_SESSION_COPY_EVERY_MS` and `BEEP_SESSION_CACHE_BYTES` lower the
    /// bounds, for tests.
    pub(super) fn locate() -> Self {
        let var = |name: &str| std::env::var(name).ok()?.parse::<u64>().ok();
        Self {
            every: var("BEEP_SESSION_COPY_EVERY_MS").map_or(EVERY, Duration::from_millis),
            cap: var("BEEP_SESSION_CACHE_BYTES").map_or(CAP, |cap| cap.min(CAP)),
            rate: RATE,
        }
    }
}

/// Copies until the node stops.
pub(super) async fn run(env: Arc<Env>) {
    loop {
        sleep(env.limits.every).await;
        if let Err(e) = pass(&env).await {
            log::debug!("background copies: {e:#}");
        }
    }
}

/// A session of this node works: copying waits.
fn busy(env: &Env) -> bool {
    env.agents.list().iter().any(|info| {
        matches!(
            info.state.status,
            Status::Starting | Status::Working | Status::Asking
        )
    })
}

async fn pass(env: &Env) -> Result<()> {
    let projects = projects_of(&env.mesh);
    let locals = locals(env).await;
    tidy(env, &locals).await;
    if projects.is_empty() {
        return Ok(());
    }
    let waits = busy(env);
    if env.agents.moves.set_paused(waits) {
        log::info!(
            "background copies {}",
            if waits {
                "wait while a session works"
            } else {
                "go on"
            }
        );
    }
    if waits {
        return Ok(());
    }
    let me = env.mesh.me();
    let peers: Vec<EndpointId> = env
        .mesh
        .devices()
        .into_iter()
        .filter(|status| status.online && status.device.id != me)
        .map(|status| status.device.id)
        .collect();
    let mut fetched = 0;
    for peer in peers {
        if busy(env) || fetched >= MAX_FETCHES {
            break;
        }
        if let Err(e) = from_peer(env, &projects, &locals, peer, &mut fetched).await {
            log::debug!("background copies from {peer}: {e:#}");
        }
    }
    Ok(())
}

/// Takes away the copies that are too old, or that this machine has a
/// session of its own for.
async fn tidy(env: &Env, locals: &Locals) {
    let cache = env.cache.clone();
    let Ok(entries) = spawn_blocking(move || cache.entries()).await else {
        return;
    };
    let now = now_secs();
    for entry in entries {
        let Some(copy) = &entry.copy else {
            // A pull that stopped before it noted anything is picked up by
            // the next one; one left for a day is not.
            if entry.copy_bytes > 0
                && env
                    .cache
                    .idle_secs(&entry.agent, &entry.id)
                    .is_some_and(|idle| idle > PARTIAL_FOR.as_secs())
            {
                let cache = env.cache.clone();
                let _ = spawn_blocking(move || cache.remove(&entry.agent, &entry.id, false)).await;
            }
            continue;
        };
        let held_here = locals.has(&entry.id);
        if held_here || !is_recent(copy.last_activity, now) {
            let cache = env.cache.clone();
            let _ = spawn_blocking(move || cache.remove(&entry.agent, &entry.id, false)).await;
        }
    }
}

async fn from_peer(
    env: &Env,
    projects: &[Project],
    locals: &Locals,
    peer: EndpointId,
    fetched: &mut usize,
) -> Result<()> {
    let connection = dial(env, peer).await?;
    let offers = {
        let (send, recv) = connection.open_bi().await?;
        let (mut tx, mut rx) = link(send, recv);
        tx.send(&Request::List {}).await?;
        match rx.recv::<Reply>().await? {
            Some(Reply::Offers { offers }) => offers,
            Some(Reply::Refused { message }) => bail!("it refused the list: {message}"),
            _ => bail!("it did not answer the list"),
        }
    };
    let now = now_secs();
    for offer in offers {
        if *fetched >= MAX_FETCHES || busy(env) {
            break;
        }
        if harness_of(offer.agent.as_str()).is_none() {
            continue;
        }
        if !is_recent(offer.last_activity, now) || !projects.iter().any(|p| p.name == offer.project)
        {
            continue;
        }
        let done = match locals.get(&offer.id) {
            Some(local) => settle(env, peer, &offer, local).await,
            None => fetch(env, &connection, peer, &offer, fetched).await,
        };
        if let Err(e) = done {
            log::debug!("session {} not copied from {peer}: {e:#}", offer.id);
        }
    }
    connection.close(0u32.into(), b"done");
    Ok(())
}

/// Brings the copy of `offer` up to date.
async fn fetch(
    env: &Env,
    connection: &iroh::endpoint::Connection,
    peer: EndpointId,
    offer: &Offer,
    fetched: &mut usize,
) -> Result<()> {
    let Some(copy) = env.cache.copy(&offer.agent, &offer.id) else {
        bail!("not a session name");
    };
    if let Some(meta) = env.cache.meta(&offer.agent, &offer.id)
        && is_held(&meta, offer, peer)
    {
        return Ok(());
    }
    let evict = make_room(env, offer, &copy).await?;
    if let Some(dir) = copy.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    let (send, recv) = connection.open_bi().await?;
    let (tx, rx) = link(send, recv);
    let pulled = pull(
        tx,
        rx,
        Ask {
            agent: offer.agent.clone(),
            id: &offer.id,
            copy: &copy,
            kind: Kind::Background {
                rate: env.limits.rate,
            },
        },
    )
    .await?;
    *fetched += 1;
    // Only a pull that went through makes room: a failed one costs nothing.
    let cache = env.cache.clone();
    spawn_blocking(move || {
        for (agent, id) in &evict {
            cache.remove(agent, id, false);
        }
    })
    .await?;
    let hash = hash_of(&copy).await?;
    let meta = Meta {
        from: peer,
        project: offer.project.clone(),
        path: offer.path.clone(),
        file_name: pulled.file_name,
        size: pulled.size,
        hash,
        last_activity: offer.last_activity,
        fetched_at: now_secs(),
    };
    let (cache, agent, id) = (env.cache.clone(), offer.agent.clone(), offer.id.clone());
    spawn_blocking(move || cache.save_meta(&agent, &id, &meta)).await??;
    if pulled.restarted {
        // The older copy kept beside the new one counts against the cap.
        trim_after_restart(env, offer).await?;
    }
    Ok(())
}

/// Whether the copy noted by `meta` is as good as `offer` from `peer`, or
/// better: a session continued on another machine has bytes of its own
/// (its folders are rewritten), so a machine does not flip between two
/// holders but keeps the later one.
fn is_held(meta: &Meta, offer: &Offer, peer: EndpointId) -> bool {
    match offer.last_activity.cmp(&meta.last_activity) {
        Ordering::Less => true,
        Ordering::Equal => {
            meta.from != peer || (meta.size == offer.size && meta.hash == offer.hash)
        }
        Ordering::Greater => false,
    }
}

/// After a pull that started over: the cache holds the new copy and the old
/// one beside it. Older copies of other sessions go to bring it under the
/// cap, and the old one with them if that is not enough.
async fn trim_after_restart(env: &Env, offer: &Offer) -> Result<()> {
    let (cache, cap) = (env.cache.clone(), env.limits.cap);
    let (agent, id, activity) = (offer.agent.clone(), offer.id.clone(), offer.last_activity);
    spawn_blocking(move || {
        let entries = cache.entries();
        let own = Some((&agent, id.as_str()));
        match evictions(&entries, cap, 0, activity, own) {
            Some(gone) => {
                for (other, key) in &gone {
                    cache.remove(other, key, false);
                }
            }
            None => cache.drop_diverged(&agent, &id),
        }
    })
    .await?;
    Ok(())
}

/// The oldest copies to take away once the bytes `offer` adds are here; fails
/// when they could not fit even then.
async fn make_room(env: &Env, offer: &Offer, copy: &Path) -> Result<Vec<(AgentSlug, String)>> {
    let held = tokio::fs::metadata(copy).await.map_or(0, |meta| meta.len());
    // The note kept beside the copy counts too.
    let need = offer.size.saturating_sub(held) + NOTE_ROOM;
    let (cache, cap, activity) = (env.cache.clone(), env.limits.cap, offer.last_activity);
    let (agent, id) = (offer.agent.clone(), offer.id.clone());
    let gone = spawn_blocking(move || {
        let entries: Vec<Entry> = cache.entries();
        evictions(&entries, cap, need, activity, Some((&agent, id.as_str())))
    })
    .await?;
    match gone {
        Some(gone) => Ok(gone),
        None => {
            log::info!("not copying session {}: the cache is full", offer.id);
            bail!("the cache is full")
        }
    }
}

/// A session this machine has a transcript of already, and another machine
/// offers too. When the user continued it here from a copy and the other
/// machine went on after the point the copy was taken (two machines
/// continued it), the one with the later activity wins; if that is the
/// other, this machine's version is kept as a recoverable copy, taken out
/// of its store. A session this machine never continued stays as it is.
async fn settle(env: &Env, peer: EndpointId, offer: &Offer, local: &Path) -> Result<()> {
    let (cache, agent, id) = (env.cache.clone(), offer.agent.clone(), offer.id.clone());
    let note = spawn_blocking(move || cache.continued(&agent, &id)).await?;
    let Some(note) = note.filter(|note| offer.last_activity > note.base) else {
        return Ok(());
    };
    // A move still going, or made after the user continued the session
    // here, names the owner: the record decides, not which version is
    // later. One that ended before is history, and two machines that went
    // on since still settle. A session that may be coming back from a
    // restart is not touched either.
    if let Some(record) = env
        .mesh
        .get(&Key::SessionMove(offer.id.clone()))
        .and_then(|bytes| SessionMove::from_bytes(&bytes).ok())
    {
        // Only the two ends of a move keep its state: for a machine that
        // is neither, an old record is history, not an unfinished move.
        let me = env.mesh.me();
        let unfinished = (record.from == me || record.to == me)
            && env
                .state
                .load(&offer.id)
                .is_none_or(|saved| !saved.is_over(record.at));
        if unfinished || record.at / 1000 > note.at {
            log::info!("session {}: not settled, a move names it", offer.id);
            return Ok(());
        }
    }
    if !env.agents.reopened() || env.agents.runs(&offer.id) || env.agents.moves.holds(&offer.id) {
        return Ok(());
    }
    let (len, modified) = {
        let local = local.to_owned();
        spawn_blocking(move || -> Result<_> {
            let meta = fs::metadata(&local)?;
            Ok((meta.len(), unix_secs(&meta)))
        })
        .await??
    };
    let me = env.mesh.me();
    if (modified, me.as_bytes()) > (offer.last_activity, peer.as_bytes()) {
        return Ok(());
    }
    log::info!(
        "session {} went two ways; the version on {} is later, this machine's is kept as a copy",
        offer.id,
        env.name_of(&peer)
    );
    keep_as_lost(env, offer, local, len, modified).await?;
    let (cache, agent, id) = (env.cache.clone(), offer.agent.clone(), offer.id.clone());
    spawn_blocking(move || cache.clear_continued(&agent, &id)).await?;
    Ok(())
}

async fn keep_as_lost(
    env: &Env,
    offer: &Offer,
    local: &Path,
    len: u64,
    modified: u64,
) -> Result<()> {
    keep_local(
        env,
        &offer.agent,
        &offer.id,
        &Kept {
            project: &offer.project,
            path: &offer.path,
            len,
            modified,
        },
        local,
    )
    .await
    .map(|_| ())
}

/// What is known of a transcript kept as a version that lost: the project
/// and folder its session ran in, its length and its last write (unix
/// seconds).
pub(super) struct Kept<'a> {
    pub(super) project: &'a str,
    pub(super) path: &'a str,
    pub(super) len: u64,
    pub(super) modified: u64,
}

/// Moves the transcript at `local` out of its harness's store into the cache
/// as a version that lost, where the list shows it.
pub(super) async fn keep_local(
    env: &Env,
    agent: &AgentSlug,
    id: &str,
    kept: &Kept<'_>,
    local: &Path,
) -> Result<u64> {
    let hash = hash_of(local).await?;
    let file_name = local
        .file_name()
        .and_then(|name| name.to_str())
        .context("the transcript has no file name")?
        .to_owned();
    let meta = Meta {
        from: env.mesh.me(),
        project: kept.project.to_owned(),
        path: kept.path.to_owned(),
        file_name,
        size: kept.len,
        hash,
        last_activity: kept.modified,
        fetched_at: now_secs(),
    };
    let (cache, agent, id, local) = (
        env.cache.clone(),
        agent.clone(),
        id.to_owned(),
        local.to_owned(),
    );
    let at = now_secs();
    Ok(spawn_blocking(move || cache.keep_lost(&agent, &id, &local, &meta, at)).await??)
}

pub(super) fn unix_secs(meta: &fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_secs())
}

/// The blake3 hash of the file at `path`.
async fn hash_of(path: &Path) -> Result<Vec<u8>> {
    let path = path.to_owned();
    spawn_blocking(move || hash_file(&path)).await?
}

fn hash_file(path: &Path) -> Result<Vec<u8>> {
    let mut hasher = blake3::Hasher::new();
    let mut reader = File::open(path)?;
    let mut buf = vec![0; 256 * 1024];
    loop {
        let got = reader.read(&mut buf)?;
        if got == 0 {
            return Ok(hasher.finalize().as_bytes().to_vec());
        }
        hasher.update(&buf[..got]);
    }
}

/// The copies in the cache and the versions that lost, as `Copies` lists
/// them.
pub(crate) async fn views(env: &Env) -> Vec<CopyView> {
    let cache = env.cache.clone();
    spawn_blocking(move || {
        let mut out = Vec::new();
        for entry in cache.entries() {
            let view = |meta: &Meta, lost: Option<u64>| CopyView {
                agent: entry.agent.to_string(),
                id: entry.id.clone(),
                from: meta.from,
                project: meta.project.clone(),
                path: meta.path.clone(),
                last_activity: meta.last_activity,
                fetched_at: meta.fetched_at,
                size: meta.size,
                lost,
            };
            if let Some(meta) = &entry.copy {
                out.push(view(meta, None));
            }
            for at in &entry.lost {
                if let Some((_, meta)) = cache.lost(&entry.agent, &entry.id, *at) {
                    out.push(view(&meta, Some(*at)));
                }
            }
        }
        out
    })
    .await
    .unwrap_or_default()
}

/// Throws away a copy, or the version that lost at `lost`.
pub(crate) async fn discard(env: &Env, agent: &AgentSlug, id: &str, lost: Option<u64>) {
    let (cache, agent, id) = (env.cache.clone(), agent.clone(), id.to_owned());
    let done = spawn_blocking(move || match lost {
        Some(at) => cache.drop_lost(&agent, &id, at),
        None => cache.remove(&agent, &id, false),
    })
    .await;
    if let Err(e) = done {
        log::warn!("a copy was not discarded: {e}");
    }
}

/// The transcripts of the sessions this machine has in its harnesses' stores,
/// by session id: read once for a whole pass.
struct Locals(HashMap<String, PathBuf>);

impl Locals {
    fn get(&self, id: &str) -> Option<&Path> {
        self.0.get(id).map(PathBuf::as_path)
    }

    fn has(&self, id: &str) -> bool {
        self.0.contains_key(id)
    }
}

async fn locals(env: &Env) -> Locals {
    let seen = env.agents.sightings().await.unwrap_or_default();
    Locals(
        seen.into_iter()
            .filter(|one| harness_of(one.agent.as_str()).is_some())
            .filter_map(|one| Some((one.id, one.file?)))
            .collect(),
    )
}
