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
use beep_proto::{
    AgentSlug,
    node::{
        agent::{CopyView, Status},
        link,
    },
};
use iroh::EndpointId;
use std::{
    fs::{self, File},
    io::Read,
    path::Path,
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
    tidy(env).await;
    if projects.is_empty() || busy(env) {
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
        if let Err(e) = from_peer(env, &projects, peer, &mut fetched).await {
            log::debug!("background copies from {peer}: {e:#}");
        }
    }
    Ok(())
}

/// Takes away the copies that are too old, or that this machine has a
/// session of its own for.
async fn tidy(env: &Env) {
    let cache = env.cache.clone();
    let Ok(entries) = spawn_blocking(move || cache.entries()).await else {
        return;
    };
    let now = now_secs();
    for entry in entries {
        let Some(copy) = &entry.copy else {
            continue;
        };
        let held_here = match harness_of(entry.agent.as_str()) {
            Some(harness) => env
                .agents
                .transcript_file(harness, &entry.id)
                .await
                .is_some(),
            None => false,
        };
        if held_here || !is_recent(copy.last_activity, now) {
            let cache = env.cache.clone();
            let _ = spawn_blocking(move || cache.remove(&entry.agent, &entry.id, false)).await;
        }
    }
}

async fn from_peer(
    env: &Env,
    projects: &[Project],
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
        let Some(harness) = harness_of(offer.agent.as_str()) else {
            continue;
        };
        if !is_recent(offer.last_activity, now) || !projects.iter().any(|p| p.name == offer.project)
        {
            continue;
        }
        let local = env.agents.transcript_file(harness, &offer.id).await;
        let done = match local {
            Some(local) => settle(env, peer, &offer, &local).await,
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
    if env
        .cache
        .meta(&offer.agent, &offer.id)
        .is_some_and(|meta| meta.size == offer.size && meta.hash == offer.hash)
    {
        return Ok(());
    }
    make_room(env, offer, &copy).await?;
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
    Ok(())
}

/// Takes away the oldest copies until the bytes `offer` adds fit; fails when
/// they cannot.
async fn make_room(env: &Env, offer: &Offer, copy: &Path) -> Result<()> {
    let held = tokio::fs::metadata(copy).await.map_or(0, |meta| meta.len());
    // The note kept beside the copy counts too.
    let need = offer.size.saturating_sub(held) + NOTE_ROOM;
    let (cache, cap, activity) = (env.cache.clone(), env.limits.cap, offer.last_activity);
    let gone = spawn_blocking(move || {
        let entries: Vec<Entry> = cache.entries();
        let gone = evictions(&entries, cap, need, activity)?;
        for (agent, id) in &gone {
            cache.remove(agent, id, false);
        }
        Some(gone.len())
    })
    .await?;
    match gone {
        Some(_) => Ok(()),
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
    let base = spawn_blocking(move || cache.continued(&agent, &id)).await?;
    if !base.is_some_and(|base| offer.last_activity > base) {
        return Ok(());
    }
    if env.agents.runs(&offer.id) || env.agents.moves.holds(&offer.id) {
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
    let hash = hash_of(local).await?;
    let file_name = local
        .file_name()
        .and_then(|name| name.to_str())
        .context("the transcript has no file name")?
        .to_owned();
    let meta = Meta {
        from: env.mesh.me(),
        project: offer.project.clone(),
        path: offer.path.clone(),
        file_name,
        size: len,
        hash,
        last_activity: modified,
        fetched_at: now_secs(),
    };
    let (cache, agent, id, local) = (
        env.cache.clone(),
        offer.agent.clone(),
        offer.id.clone(),
        local.to_owned(),
    );
    let at = now_secs();
    spawn_blocking(move || cache.keep_lost(&agent, &id, &local, &meta, at)).await??;
    Ok(())
}

fn unix_secs(meta: &fs::Metadata) -> u64 {
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
