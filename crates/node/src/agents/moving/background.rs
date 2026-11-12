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
    cache::{Cache, Entry, Meta, evictions},
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
use futures::{StreamExt, stream::FuturesUnordered};
use iroh::{EndpointId, endpoint::Connection};
use std::{
    cmp::{Ordering, Reverse},
    collections::HashMap,
    fs::{self, File},
    hash::{DefaultHasher, Hash, Hasher},
    io::Read,
    path::Path,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
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
/// How long passes are skipped after one that found no room, when nothing
/// it saw changed: copies age out and sessions appear here without a sign.
const RECHECK: Duration = Duration::from_secs(10 * 60);

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
    let mut stalled = None;

    loop {
        sleep(env.limits.every).await;

        stalled = match pass(&env, stalled).await {
            Ok(next) => next,
            Err(e) => {
                log::debug!("background copies: {e:#}");
                None
            }
        };
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

/// What a pass's work depends on: the sessions offered, the cache and its
/// cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Seen {
    /// A hash of the offers worth copying, with the machine of each.
    offers: u64,
    /// The cache's count of changes.
    changes: u64,
    cap: u64,
}

/// A pass left sessions out for want of room, having seen `seen`.
#[derive(Debug, Clone, Copy)]
struct Stalled {
    seen: Seen,
    at: Instant,
}

impl Stalled {
    /// A pass that would see `seen` at `now` would find no room either.
    fn skips(&self, seen: &Seen, now: Instant) -> bool {
        self.seen == *seen && now.duration_since(self.at) < RECHECK
    }
}

/// The offers of one machine, with the connection they came on.
struct Listed {
    peer: EndpointId,
    connection: Connection,
    offers: Vec<Offer>,
}

/// One pass; returns what it saw when it left sessions out for want of room,
/// for the next to skip while nothing changes.
async fn pass(env: &Env, stalled: Option<Stalled>) -> Result<Option<Stalled>> {
    let projects = projects_of(&env.mesh);

    // With no project to copy for, the session stores are not walked; old
    // copies still go.
    if projects.is_empty() {
        let mut room = Room::read(env).await?;
        tidy(env, &mut room, &Locals(HashMap::new())).await?;
        return Ok(None);
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
        let locals = locals(env).await;
        let mut room = Room::read(env).await?;
        tidy(env, &mut room, &locals).await?;
        return Ok(stalled);
    }

    let listed = list(env, &projects).await;
    let seen = Seen {
        offers: offers_hash(&listed),
        changes: env.cache.changes(),
        cap: env.limits.cap,
    };

    if stalled.is_some_and(|stalled| stalled.skips(&seen, Instant::now())) {
        close(&listed);
        return Ok(stalled);
    }

    let locals = locals(env).await;
    let mut room = Room::read(env).await?;
    tidy(env, &mut room, &locals).await?;
    let mut queue: Vec<(&Listed, &Offer)> = listed
        .iter()
        .flat_map(|one| one.offers.iter().map(move |offer| (one, offer)))
        .collect();
    // Newest first: once one finds no room, none after it can.
    queue.sort_by_key(|(_, offer)| Reverse(offer.last_activity));
    let mut fetched = 0;
    let mut no_room = 0;
    let mut cut_short = false;

    for (one, offer) in queue {
        if busy(env) || fetched >= MAX_FETCHES {
            cut_short = true;
            break;
        }

        let done = match locals.get(&offer.id) {
            Some(local) => settle(env, one.peer, offer, local).await,
            None => fetch(env, &mut room, &one.connection, one.peer, offer, &mut fetched).await,
        };

        match done {
            Err(e) if e.is::<CacheFull>() => no_room += 1,
            Err(e) => log::debug!("session {} not copied from {}: {e:#}", offer.id, one.peer),
            Ok(()) => {}
        }
    }

    close(&listed);

    if no_room == 0 || cut_short {
        return Ok(None);
    }

    log::info!("not copying {no_room} sessions: the cache is full");
    Ok(Some(Stalled {
        seen: Seen {
            changes: env.cache.changes(),
            ..seen
        },
        at: Instant::now(),
    }))
}

/// The offers worth copying of every other machine online, asked together.
async fn list(env: &Env, projects: &[Project]) -> Vec<Listed> {
    let me = env.mesh.me();
    let now = now_secs();
    let mut asked: FuturesUnordered<_> = env
        .mesh
        .code_hosts()
        .into_iter()
        .filter(|status| status.online && status.device.id != me)
        .map(|status| async move {
            let peer = status.device.id;
            (peer, list_one(env, peer).await)
        })
        .collect();
    let mut listed = Vec::new();

    while let Some((peer, got)) = asked.next().await {
        let (connection, mut offers) = match got {
            Ok(got) => got,
            Err(e) => {
                log::debug!("background copies from {peer}: {e:#}");
                continue;
            }
        };
        offers.retain(|offer| {
            harness_of(offer.agent.as_ref()).is_some()
                && is_recent(offer.last_activity, now)
                && projects.iter().any(|p| p.name == offer.project)
        });
        listed.push(Listed {
            peer,
            connection,
            offers,
        });
    }

    // In one order whichever machine answers first, for `offers_hash`.
    listed.sort_by_key(|one| *one.peer.as_bytes());
    listed
}

async fn list_one(env: &Env, peer: EndpointId) -> Result<(Connection, Vec<Offer>)> {
    let connection = dial(env, peer).await?;
    let (send, recv) = connection.open_bi().await?;
    let (mut tx, mut rx) = link(send, recv);
    tx.send(&Request::List {}).await?;

    match rx.recv::<Reply>().await? {
        Some(Reply::Offers { offers }) => Ok((connection, offers)),
        Some(Reply::Refused { message }) => bail!("it refused the list: {message}"),
        _ => bail!("it did not answer the list"),
    }
}

fn offers_hash(listed: &[Listed]) -> u64 {
    let mut hasher = DefaultHasher::new();

    for one in listed {
        one.peer.as_bytes().hash(&mut hasher);

        for offer in &one.offers {
            let Offer {
                agent,
                id,
                project,
                path,
                last_activity,
                size,
                hash,
            } = offer;
            (agent, id, project, path, last_activity, size, hash).hash(&mut hasher);
        }
    }

    hasher.finish()
}

fn close(listed: &[Listed]) {
    for one in listed {
        one.connection.close(0u32.into(), b"done");
    }
}

/// The cache as one pass sees it: read once when the pass starts, then kept
/// up to date with what the pass changes, so no session reads it again.
struct Room {
    entries: Vec<Entry>,
    cap: u64,
    /// A session found no room, and none as old as it can either.
    full: bool,
}

impl Room {
    fn new(entries: Vec<Entry>, cap: u64) -> Self {
        Self {
            entries,
            cap,
            full: false,
        }
    }

    async fn read(env: &Env) -> Result<Self> {
        let cache = env.cache.clone();
        let entries = spawn_blocking(move || cache.entries()).await?;
        Ok(Self::new(entries, env.limits.cap))
    }

    fn get(&self, agent: &AgentSlug, id: &str) -> Option<&Entry> {
        self.entries
            .iter()
            .find(|entry| entry.agent == *agent && entry.id == id)
    }

    /// The oldest copies to take away once the bytes `offer` adds are here;
    /// fails when they could not fit even then. Once not even a note fits
    /// after every copy older than `offer` goes, it fails for every session.
    fn make_room(&mut self, offer: &Offer) -> Result<Vec<(AgentSlug, String)>, CacheFull> {
        if self.full {
            return Err(CacheFull);
        }

        let held = self
            .get(&offer.agent, &offer.id)
            .map_or(0, |entry| entry.copy_len);
        // The note kept beside the copy counts too.
        let need = offer.size.saturating_sub(held) + NOTE_ROOM;
        let own = Some((&offer.agent, offer.id.as_str()));

        if let Some(gone) = evictions(&self.entries, self.cap, need, offer.last_activity, own) {
            return Ok(gone);
        }

        // Too big alone, or the cache is full for its age; a session no newer
        // needs at least a note's room and may take away no more.
        self.full = evictions(&self.entries, self.cap, NOTE_ROOM, offer.last_activity, None)
            .is_none();
        Err(CacheFull)
    }

    /// Takes in what the cache now holds of the sessions `keys`: `fresh`,
    /// read again from the disk, for those that kept a folder.
    fn replace(&mut self, keys: &[(AgentSlug, String)], fresh: Vec<Entry>) {
        self.entries
            .retain(|entry| !keys.iter().any(|(agent, id)| entry.agent == *agent && entry.id == *id));
        self.entries.extend(fresh);
    }
}

/// Takes away the copies of the sessions `gone`, and reads again what is
/// left of them: a version that lost stays.
fn evict(cache: &Cache, gone: &[(AgentSlug, String)]) -> Vec<Entry> {
    gone.iter()
        .filter_map(|(agent, id)| {
            cache.remove(agent, id, false);
            cache.entry(agent, id)
        })
        .collect()
}

/// Takes away the copies that are too old, or that this machine has a
/// session of its own for.
async fn tidy(env: &Env, room: &mut Room, locals: &Locals) -> Result<()> {
    let now = now_secs();
    let mut gone = Vec::new();
    let mut partial = Vec::new();

    for entry in &room.entries {
        let key = (entry.agent.clone(), entry.id.clone());

        match &entry.copy {
            Some(copy) if locals.has(&entry.id) || !is_recent(copy.last_activity, now) => {
                gone.push(key);
            }
            Some(_) => {}
            // A pull that stopped before it noted anything is picked up by
            // the next one; one left for a day is not.
            None if entry.copy_bytes > 0 => partial.push(key),
            None => {}
        }
    }

    if gone.is_empty() && partial.is_empty() {
        return Ok(());
    }

    let cache = env.cache.clone();
    let (gone, fresh) = spawn_blocking(move || {
        gone.extend(partial.into_iter().filter(|(agent, id)| {
            cache
                .idle_secs(agent, id)
                .is_some_and(|idle| idle > PARTIAL_FOR.as_secs())
        }));
        let fresh = evict(&cache, &gone);
        (gone, fresh)
    })
    .await?;
    room.replace(&gone, fresh);
    Ok(())
}

/// Brings the copy of `offer` up to date.
async fn fetch(
    env: &Env,
    room: &mut Room,
    connection: &Connection,
    peer: EndpointId,
    offer: &Offer,
    fetched: &mut usize,
) -> Result<()> {
    let Some(copy) = env.cache.copy(&offer.agent, &offer.id) else {
        bail!("not a session name");
    };

    if let Some(meta) = room
        .get(&offer.agent, &offer.id)
        .and_then(|entry| entry.copy.as_ref())
        && is_held(meta, offer, peer)
    {
        return Ok(());
    }

    let evicted = room.make_room(offer)?;

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
    // Only a pull that went through makes room: a failed one costs nothing.
    let (keys, fresh) = spawn_blocking(move || -> std::io::Result<_> {
        let mut fresh = evict(&cache, &evicted);
        cache.save_meta(&agent, &id, &meta)?;
        fresh.extend(cache.entry(&agent, &id));
        let mut keys = evicted;
        keys.push((agent, id));
        Ok((keys, fresh))
    })
    .await??;
    room.replace(&keys, fresh);

    if pulled.restarted {
        // The older copy kept beside the new one counts against the cap.
        trim_after_restart(env, room, offer).await?;
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
async fn trim_after_restart(env: &Env, room: &mut Room, offer: &Offer) -> Result<()> {
    let own = Some((&offer.agent, offer.id.as_str()));
    let gone = evictions(&room.entries, room.cap, 0, offer.last_activity, own);
    let (cache, agent, id) = (env.cache.clone(), offer.agent.clone(), offer.id.clone());
    let (keys, fresh) = spawn_blocking(move || match gone {
        Some(gone) => {
            let fresh = evict(&cache, &gone);
            (gone, fresh)
        }
        None => {
            cache.drop_diverged(&agent, &id);
            let fresh = cache.entry(&agent, &id).into_iter().collect();
            (vec![(agent, id)], fresh)
        }
    })
    .await?;
    room.replace(&keys, fresh);
    Ok(())
}

/// No room in the cache for a copy, even after taking away older ones.
#[derive(Debug, PartialEq, Eq)]
struct CacheFull;

impl std::fmt::Display for CacheFull {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the cache is full")
    }
}

impl std::error::Error for CacheFull {}

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
            .filter(|one| harness_of(one.agent.as_ref()).is_some())
            .filter_map(|one| Some((one.id, one.file?)))
            .collect(),
    )
}

#[cfg(test)]
#[path = "../../tests/agents_moving_background.rs"]
mod tests;
