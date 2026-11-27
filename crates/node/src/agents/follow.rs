//! Pushes an initial snapshot and subsequent changes to an open session view.

use super::{Agents, limits::MAX_TRANSCRIPT};
use crate::logs::claude::Followed;
use crate::logs::found::Agent;
use crate::logs::transcript_stage;
use crate::events::HEARTBEAT;
use anyhow::{Context, Result, bail};
use beep_proto::SessionId;
use beep_proto::node::{
    LinkRx, LinkTx,
    agent::{
        Entry, EntryKind, EntryPatch, ExternalSession, FOLLOW_READERS_FULL, FollowControl,
        FollowSession, SessionCursor, SessionPatch, SessionSnapshot, SessionSource, SessionUpdate,
        Status,
    },
};
use beep_proto::watched;
use std::{
    collections::{BTreeSet, HashMap},
    future::pending,
    ops::Range,
    path::PathBuf,
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    sync::{Semaphore, SemaphorePermit, mpsc},
    task::spawn_blocking,
    time::{Instant, MissedTickBehavior, interval_at, sleep, timeout},
};

/// Every desktop window and phone shares these; a desktop window may hold
/// several sessions. A refused reader hears `FOLLOW_READERS_FULL`.
static READERS: Semaphore = Semaphore::const_new(128);
const RETAINED_ENTRIES: usize = 5000;
/// Most raw entries read for an opening view when consecutive tool and thought
/// entries fold into single progress rows.
const INITIAL_ENTRIES: usize = 1000;
// Leave room for source metadata inside the transport's 8 MiB frame limit.
const RETAINED_BYTES: usize = 4 * 1024 * 1024;
/// The most all readers' cached windows take together. Past it the least
/// recently updated windows drop their cache and re-encode on their next update.
const CACHED_BYTES: usize = 64 * 1024 * 1024;

static WINDOWS: LazyLock<Mutex<Windows>> = LazyLock::new(Mutex::default);

const WRITE_WAIT: Duration = Duration::from_secs(15);
/// How long a follow gathers changes before reading again: briefly for a
/// session Beep runs, longer for one read from its transcript file, and
/// slower when the client reports the session is off screen or hidden.
const COALESCE_LIVE: Duration = Duration::from_millis(40);
const COALESCE_FILE: Duration = Duration::from_millis(200);
const COALESCE_BACKGROUND: Duration = Duration::from_secs(1);

pub(super) mod observers;
mod sources;
pub(crate) use observers::Sources;

#[derive(Debug, PartialEq)]
struct Summary {
    id: SessionId,
    agent: String,
    title: Option<String>,
    cwd: String,
    presence: beep_proto::node::agent::Presence,
    pid: Option<u32>,
}

/// Directory listings belong to the notification lease. Other harnesses' writes
/// do not invalidate them; descriptions still apply current presence and folders.
#[derive(Default)]
struct SummaryCache {
    claude: Option<Vec<ExternalSession>>,
}

impl SummaryCache {
    /// The summary of every session, after the occupancy read the stores
    /// again for the agents in `changed`.
    async fn read(&mut self, agents: &Agents, changed: &BTreeSet<Agent>) -> Result<Vec<Summary>> {
        let sighted = agents.occupancy.refresh(changed).await?;
        let stores = agents.stores().clone();
        let refresh_claude = self.claude.is_none() || changed.contains(&Agent::Claude);
        let described = spawn_blocking(move || {
            // Preserve the stores' order when activity times tie.
            let mut seen: Vec<_> = sighted
                .seen
                .iter()
                .filter(|session| session.agent != Agent::Claude)
                .cloned()
                .collect();
            seen.sort_by_key(|session| std::cmp::Reverse(session.last_activity));
            let now = std::time::SystemTime::now();
            seen.into_iter()
                .take(512)
                .map(|session| stores.describe(session, now))
                .collect::<Vec<_>>()
        });
        let claude = async {
            if refresh_claude {
                agents
                    .external()
                    .list(512, None, agents.sessions.owned())
                    .await
                    .map(Some)
            } else {
                Ok(None)
            }
        };
        let (claude, others) = tokio::join!(claude, described);
        let others = others.context("reading agent summary")?;

        match claude {
            Ok(Some(claude)) => self.claude = Some(claude),
            Ok(None) => {}
            Err(error) => {
                self.claude = None;
                return Err(error);
            }
        }

        let mut sessions = self.claude.clone().unwrap_or_default();
        agents.observe_external(&sessions);
        sessions.extend(others);
        agents.occupancy.presence().merge(&mut sessions);
        agents.occupancy.settle_closed(&mut sessions);
        let mut summary: Vec<_> = sessions
            .into_iter()
            .map(|session| Summary {
                id: session.id,
                agent: session.agent,
                title: session.title,
                cwd: session.cwd,
                presence: session.presence,
                pid: session.pid,
            })
            .collect();
        summary.sort_by(|a, b| (&a.agent, &a.id).cmp(&(&b.agent, &b.id)));
        Ok(summary)
    }
}

#[cfg(test)]
async fn source_summary(agents: &Agents) -> Result<Vec<Summary>> {
    SummaryCache::default()
        .read(agents, &Agent::ALL.into_iter().collect())
        .await
}

pub async fn serve_follow(mut tx: LinkTx, mut rx: LinkRx, agents: Arc<Agents>) -> Result<()> {
    let request: FollowSession = timeout(WRITE_WAIT, rx.recv())
        .await??
        .context("session subscription closed before its request")?;
    let Ok(permit) = READERS.try_acquire() else {
        tracing::info!(session = %request.session, "session follow refused because all readers are in use");
        tx.send(&SessionUpdate::Error {
            reason: FOLLOW_READERS_FULL.into(),
        })
        .await?;
        return Ok(());
    };
    let reader = Reader::new(permit);
    let started = Instant::now();
    let (shown_tx, shown_rx) = tokio::sync::watch::channel(!request.patches || request.shown);
    let client_rx = async {
        if !request.patches {
            return match rx.recv::<serde_json::Value>().await {
                Ok(None) => FollowEnd::ClientFinished,
                Ok(Some(_)) => FollowEnd::UnexpectedClientMessage,
                Err(error) => FollowEnd::ClientReadFailed(error),
            };
        }

        loop {
            match rx.recv::<FollowControl>().await {
                Ok(Some(FollowControl::Shown(shown))) => {
                    shown_tx.send_if_modified(|current| {
                        let changed = *current != shown;
                        *current = shown;
                        changed
                    });
                }
                Ok(None) => return FollowEnd::ClientFinished,
                Err(error) => return FollowEnd::ClientReadFailed(error),
            }
        }
    };
    // Dropping the reader cancels reads and writes too, including a blocked write.
    let end = tokio::select! {
        result = follow(&mut tx, &agents, &request, &reader, shown_rx) => result,
        reason = client_rx => Ok(reason),
    };
    let reason = end.as_ref().map_or("server_error", AsRef::as_ref);
    let error = match &end {
        Ok(FollowEnd::ClientReadFailed(error)) | Err(error) => Some(format!("{error:#}")),
        _ => None,
    };
    tracing::info!(
        session = %request.session,
        reader = reader.id,
        elapsed_ms = started.elapsed().as_millis(),
        reason,
        error,
        "session follow closed"
    );

    end.map(|_| ())
}

#[derive(strum::AsRefStr)]
#[strum(serialize_all = "snake_case")]
enum FollowEnd {
    ClientFinished,
    UnexpectedClientMessage,
    ClientReadFailed(anyhow::Error),
    SnapshotFailed,
    LiveWatchClosed,
    PresenceWatchClosed,
    FileWatchClosed,
}

async fn send(tx: &mut LinkTx, update: &SessionUpdate) -> Result<()> {
    timeout(WRITE_WAIT, tx.send(update))
        .await
        .context("session reader stopped receiving")?
}

async fn follow(
    tx: &mut LinkTx,
    agents: &Agents,
    request: &FollowSession,
    reader: &Reader,
    mut shown_rx: tokio::sync::watch::Receiver<bool>,
) -> Result<FollowEnd> {
    let mut presence = agents.occupancy.presence().subscribe();
    let mut live = agents
        .get(&request.session)
        .map(|session| session.live.clone());
    // A Claude Code session started outside Beep is found once, and the
    // machine's processes are read again only when its presence may change.
    let mut followed = if live.is_none() && request.agent == "claude" {
        agents.external().follow(&request.session).await?
    } else {
        None
    };
    let stale = Arc::new(AtomicBool::new(true));
    // Install file notifications before the initial read so a write cannot fall between them.
    let paths = paths(agents, request, followed.as_ref()).await?;
    let (dirty, mut changes) = mpsc::channel(1);
    let _watcher = if paths.is_empty() {
        None
    } else {
        let presence_path = agents.external().presence_path();
        let presence_changed = stale.clone();
        let watcher = crate::events::paths::Paths::new("agent-presence", move |roots| {
            if presence_path
                .as_ref()
                .is_some_and(|path| roots.contains(path))
            {
                presence_changed.store(true, Ordering::Release);
            }
            let _ = dirty.try_send(());
        })?;
        watcher.replace(paths).await?;
        Some(watcher)
    };
    let mut heartbeat = interval_at(Instant::now() + HEARTBEAT, HEARTBEAT);
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut observed_pid = None;
    let mut exit = None;
    // Where the reader's held entries start, once it holds some.
    let mut held = None;
    // A newly opened view reads enough entries to cover `last` folded rows at
    // once; later updates read the full retained window to validate and patch.
    let mut retained = match &request.after {
        Some(_) => RETAINED_ENTRIES,
        None => INITIAL_ENTRIES,
    };

    loop {
        watched::read_and_update(&mut presence, |_| ());

        if let Some(live) = &mut live {
            watched::read_and_update(live, |_| ());
        }

        let read = Read {
            followed: followed.as_mut(),
            stale: stale.swap(false, Ordering::AcqRel),
            retained,
            from: held,
        };
        retained = RETAINED_ENTRIES;
        let snapshot = match snapshot(agents, request, read).await {
            Ok(snapshot) => snapshot,
            Err(error) => {
                send(
                    tx,
                    &SessionUpdate::Error {
                        reason: format!("{error:#}"),
                    },
                )
                .await?;
                return Ok(FollowEnd::SnapshotFailed);
            }
        };
        let pid = match &snapshot.source {
            SessionSource::External(session) => session.pid,
            SessionSource::Owned(_) => None,
        };

        if pid != observed_pid {
            exit = pid.and_then(|pid| match crate::events::exits::Exit::watch(pid) {
                Ok(exit) => Some(exit),
                Err(error) => {
                    log::debug!("session process exit cannot be observed: {error}");
                    None
                }
            });
            observed_pid = pid;
        }

        let prior = reader.take();
        let requested = request.clone();
        let encoding = Instant::now();
        let (window, update, changed) = spawn_blocking(move || {
            let held = prior
                .as_ref()
                .map(|prior| (prior.cursor.clone(), prior.source.clone()));
            let (window, update) = next_update(snapshot, prior, &requested)?;
            let changed = held
                .is_none_or(|(cursor, source)| cursor != window.cursor || source != window.source);
            Ok::<_, anyhow::Error>((window, update, changed))
        })
        .await
        .context("preparing transcript update")??;
        transcript_stage("encode", encoding.into_std());
        let deadline = match &window.source {
            SessionSource::External(session) => recent_deadline(session),
            SessionSource::Owned(_) => None,
        };
        held = Some(window.cursor.start);
        reader.keep(window);

        if changed {
            let sending = Instant::now();
            send(tx, &update).await?;
            transcript_stage("send", sending.into_std());
        }

        // The session's sender outlives its task, so an ended session is
        // told by its state, not by the sender dropping.
        if let Some(live) = &live
            && watched::read(live, |live| live.status() == Status::Ended)
        {
            return Ok(FollowEnd::LiveWatchClosed);
        }

        let follow_presence = live.is_none();

        loop {
            tokio::select! {
                changed = async {
                    match &mut live {
                        Some(live) => live.changed().await,
                        None => pending().await,
                    }
                } => if changed.is_err() { return Ok(FollowEnd::LiveWatchClosed); },
                changed = async {
                    if follow_presence { presence.changed().await } else { pending().await }
                } => {
                    if changed.is_err() { return Ok(FollowEnd::PresenceWatchClosed); }
                    stale.store(true, Ordering::Release);
                },
                changed = changes.recv() => if changed.is_none() { return Ok(FollowEnd::FileWatchClosed); },
                _ = async {
                    match &exit {
                        Some(exit) => exit.wait().await,
                        None => pending().await,
                    }
                } => {
                    exit = None;
                    stale.store(true, Ordering::Release);
                },
                _ = async {
                    match deadline {
                        Some(deadline) => tokio::time::sleep_until(deadline).await,
                        None => pending().await,
                    }
                } => stale.store(true, Ordering::Release),
                _ = heartbeat.tick() => {
                    send(tx, &SessionUpdate::KeepAlive).await?;
                    continue;
                },
            }
            break;
        }
        // Coalesce token bursts; the watch channel and file queue retain a dirty bit.
        // A session off screen coalesces longer until it returns to screen.
        let shown_now = watched::read_and_update(&mut shown_rx, |shown| *shown);
        let coalesce = if !shown_now {
            COALESCE_BACKGROUND
        } else if live.is_some() {
            COALESCE_LIVE
        } else {
            COALESCE_FILE
        };
        let wait = sleep(coalesce);

        if shown_now {
            wait.await;
        } else {
            tokio::select! {
                _ = wait => {}
                changed = shown_rx.changed() => {
                    if changed.is_err() {
                        return Ok(FollowEnd::ClientFinished);
                    }
                }
            }
        }

        if live.is_none() {
            live = agents
                .get(&request.session)
                .map(|session| session.live.clone());
        }
    }
}

// Both log readers classify an unowned write as recent for three minutes.
// Schedule that single state transition, rather than periodically rereading idle logs.
fn recent_deadline(session: &beep_proto::node::agent::ExternalSession) -> Option<Instant> {
    if session.pid.is_some() || session.presence != beep_proto::node::agent::Presence::Recent {
        return None;
    }

    let remaining = session
        .last_activity
        .saturating_add(181)
        .saturating_sub(super::unix_now());
    Instant::now().checked_add(Duration::from_secs(remaining.max(1)))
}

/// One session reader: its slot among `READERS` and its cached window.
struct Reader {
    id: u64,
    _permit: SemaphorePermit<'static>,
}

impl Reader {
    fn new(permit: SemaphorePermit<'static>) -> Self {
        let mut windows = crate::lock(&WINDOWS);
        windows.next_id += 1;
        Self {
            id: windows.next_id,
            _permit: permit,
        }
    }

    fn take(&self) -> Option<Window> {
        crate::lock(&WINDOWS).take(self.id)
    }

    fn keep(&self, window: Window) {
        crate::lock(&WINDOWS).keep(self.id, window, CACHED_BYTES);
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        self.take();
    }
}

/// The readers' windows between updates, their caches bounded together.
#[derive(Debug, Default)]
struct Windows {
    next_id: u64,
    /// Counts updates, to find the least recently updated window.
    tick: u64,
    /// What the cached windows take together, by `Window::cost`.
    bytes: usize,
    held: HashMap<u64, (u64, Window)>,
}

impl Windows {
    fn take(&mut self, id: u64) -> Option<Window> {
        let (_, window) = self.held.remove(&id)?;
        self.bytes = self.bytes.saturating_sub(window.cost());
        Some(window)
    }

    /// Keeps reader `id`'s window, evicting the caches of the least recently
    /// updated other windows while all take more than `limit`.
    fn keep(&mut self, id: u64, window: Window, limit: usize) {
        self.tick += 1;
        self.bytes = self.bytes.saturating_add(window.cost());
        self.held.insert(id, (self.tick, window));

        while self.bytes > limit {
            let oldest = self
                .held
                .iter_mut()
                .filter(|(held, (_, window))| **held != id && window.cost() > 0)
                .min_by_key(|(_, (tick, _))| *tick);
            let Some((_, (_, window))) = oldest else {
                break;
            };

            self.bytes = self.bytes.saturating_sub(window.cost());
            window.evict();
        }
    }
}

/// A reader's retained transcript with its compact encoding, so an update encodes only
/// new or changed entries. Dropped with the follow stream.
#[derive(Debug)]
struct Window {
    source: SessionSource,
    /// Position of `entries[0]`; earlier than `cursor.start` when the reader holds less.
    earlier: u64,
    entries: Vec<Entry>,
    /// Each entry's `rmp_serde` encoding back to back, entry `i` from `starts[i]`.
    wire: Vec<u8>,
    starts: Vec<usize>,
    /// Position of `entry_hashes[0]`; survives cache eviction so per-entry diffing
    /// still works without re-sending the whole window.
    hashes_earlier: u64,
    /// Compact 64-bit BLAKE3 prefix of each entry's msgpack bytes.
    entry_hashes: Vec<u64>,
    /// Earliest entry position the client on this stream has received in full.
    known_from: u64,
    /// The range the reader holds, ending at the transcript end.
    cursor: SessionCursor,
}

impl Window {
    /// The memory its cache takes: decoded entries take about as much as
    /// their encoding.
    fn cost(&self) -> usize {
        self.wire.len().saturating_mul(2)
    }

    /// Drops the cached entries, keeping lightweight per-entry hashes so the next
    /// update can still diff individual entries instead of re-sending the whole window.
    fn evict(&mut self) {
        self.entries = Vec::new();
        self.wire = Vec::new();
        self.starts = Vec::new();
        self.earlier = self.cursor.next;
    }

    /// Whether it caches the entries the reader holds.
    fn caches_held(&self) -> bool {
        self.earlier <= self.cursor.start
    }

    fn index(&self, position: u64) -> usize {
        (position - self.earlier) as usize
    }

    fn offset(&self, index: usize) -> usize {
        self.starts.get(index).copied().unwrap_or(self.wire.len())
    }

    fn entry_at(&self, position: u64) -> Option<&Entry> {
        let index = usize::try_from(position.checked_sub(self.earlier)?).ok()?;
        self.entries.get(index)
    }

    fn hash_at(&self, position: u64) -> Option<u64> {
        let index = usize::try_from(position.checked_sub(self.hashes_earlier)?).ok()?;
        self.entry_hashes.get(index).copied()
    }

    /// Where this window encoded `entry`, if it held it unchanged at `position`.
    fn held(&self, position: u64, entry: &Entry) -> Option<(Range<usize>, u64)> {
        let index = usize::try_from(position.checked_sub(self.earlier)?).ok()?;
        let hash = self.hash_at(position)?;
        (self.entries.get(index)? == entry)
            .then(|| (self.offset(index)..self.offset(index + 1), hash))
    }

    /// Moves the reader's start within the retained entries and hashes what it holds.
    fn hold(&mut self, start: u64) -> Result<()> {
        let start = start.clamp(self.earlier, self.cursor.next);
        let index = self.index(start);
        self.cursor.start = start;
        self.cursor.fingerprint = fingerprint(
            start,
            self.entries.len() - index,
            &self.wire[self.offset(index)..],
        )?;

        if self.cursor.next > start {
            let prefix_next = self.cursor.next - 1;
            let prefix_end = self.index(prefix_next);
            self.cursor.prefix_next = prefix_next;
            self.cursor.prefix_fingerprint = fingerprint(
                start,
                prefix_end - index,
                &self.wire[self.offset(index)..self.offset(prefix_end)],
            )?;
        } else {
            self.cursor.prefix_next = start;
            self.cursor.prefix_fingerprint.clear();
        }

        Ok(())
    }

    fn suffix(&self, position: u64, resumed: bool, controls: bool) -> SessionSnapshot {
        SessionSnapshot {
            source: self.source.clone(),
            entries: self.entries[self.index(position)..].to_vec(),
            earlier: position,
            cursor: self.cursor.clone(),
            resumed,
            controls,
        }
    }
}

fn entry_hash(bytes: &[u8]) -> u64 {
    let digest = blake3::hash(bytes);
    let mut prefix = [0u8; 8];
    prefix.copy_from_slice(&digest.as_bytes()[..8]);
    u64::from_le_bytes(prefix)
}

fn append_text_suffix<'a>(old: &Entry, new: &'a Entry) -> Option<&'a str> {
    (old.kind == new.kind
        && old.ms == new.ms
        && old.tool == new.tool
        && old.sent_at == new.sent_at
        && new.text.len() > old.text.len())
    .then(|| new.text.strip_prefix(&old.text))
    .flatten()
}

/// Hash exactly the canonical window the client can retain and validate on reconnect:
/// `rmp_serde::to_vec` of `count` entries whose encodings are `wire`.
fn fingerprint(start: u64, count: usize, wire: &[u8]) -> Result<String> {
    let mut header = Vec::with_capacity(5);
    rmp::encode::write_array_len(&mut header, u32::try_from(count)?)
        .context("encoding transcript checkpoint")?;
    let length = header
        .len()
        .checked_add(wire.len())
        .context("transcript size overflow")?;
    let mut hash = blake3::Hasher::new();
    hash.update(b"session-window/1");
    hash.update(&start.to_le_bytes());
    hash.update(&u64::try_from(length)?.to_le_bytes());
    hash.update(&header);
    hash.update(wire);
    Ok(hash.finalize().to_hex().to_string())
}

fn validates_range(window: &Window, start: u64, next: u64, expected: &str) -> Result<bool> {
    if start < window.earlier || next < start || next > window.cursor.next {
        return Ok(false);
    }

    let (from, to) = (window.index(start), window.index(next));
    let wire = &window.wire[window.offset(from)..window.offset(to)];
    Ok(fingerprint(start, to - from, wire)? == expected)
}

fn validates(window: &Window, cursor: &SessionCursor) -> Result<bool> {
    validates_range(window, cursor.start, cursor.next, &cursor.fingerprint)
}

/// An entry's encoding: still in the previous window's buffer, or new.
enum Encoded {
    Held(Range<usize>, u64),
    Fresh(Vec<u8>, u64),
}

impl Encoded {
    fn len(&self) -> usize {
        match self {
            Self::Held(range, _) => range.len(),
            Self::Fresh(wire, _) => wire.len(),
        }
    }

    fn hash(&self) -> u64 {
        match self {
            Self::Held(_, hash) | Self::Fresh(_, hash) => *hash,
        }
    }
}

/// Keeps the newest entries that fit the retained limit, reusing the previous window's
/// encoding of each entry it held unchanged. Also returns which kept entries were unchanged.
fn retain(snapshot: SessionSnapshot, previous: Option<&mut Window>) -> Result<(Window, Vec<bool>)> {
    let SessionSnapshot {
        source,
        mut entries,
        earlier,
        cursor,
        ..
    } = snapshot;
    // Reserve the largest compact array header before counting complete entries.
    let mut bytes = 5usize;
    let mut encoded = Vec::new();

    for (index, entry) in entries.iter().enumerate().rev() {
        let position = earlier
            .checked_add(u64::try_from(index)?)
            .context("transcript position overflow")?;
        let entry = match previous
            .as_deref()
            .and_then(|previous| previous.held(position, entry))
        {
            Some((range, hash)) => Encoded::Held(range, hash),
            None => {
                let bytes = rmp_serde::to_vec(entry).context("measuring transcript entry")?;
                let hash = entry_hash(&bytes);
                Encoded::Fresh(bytes, hash)
            }
        };
        let next = bytes
            .checked_add(entry.len())
            .context("transcript size overflow")?;
        if next > RETAINED_BYTES {
            break;
        }
        bytes = next;
        encoded.push(entry);
    }

    if encoded.is_empty() && !entries.is_empty() {
        bail!("a transcript entry exceeds the retained window limit");
    }

    encoded.reverse();
    let unchanged = encoded
        .iter()
        .map(|entry| matches!(entry, Encoded::Held(_, _)))
        .collect();
    let entry_hashes = encoded.iter().map(Encoded::hash).collect();
    let mut starts = Vec::with_capacity(encoded.len());
    let mut offset = 0;

    for entry in &encoded {
        starts.push(offset);
        offset += entry.len();
    }

    let previous_wire = previous
        .map(|previous| std::mem::take(&mut previous.wire))
        .unwrap_or_default();
    let wire = assemble(encoded, previous_wire, offset);
    let removed = entries.len() - starts.len();
    entries.drain(..removed);
    let earlier = earlier
        .checked_add(u64::try_from(removed)?)
        .context("transcript position overflow")?;
    let window = Window {
        source,
        earlier,
        entries,
        wire,
        starts,
        hashes_earlier: earlier,
        entry_hashes,
        known_from: earlier,
        cursor: SessionCursor {
            start: earlier,
            ..cursor
        },
    };
    Ok((window, unchanged))
}

/// Lays the kept encodings out back to back. While a stream only appends or edits its
/// last entries, the unchanged ones are one run of the previous buffer, trimmed in place.
fn assemble(encoded: Vec<Encoded>, previous: Vec<u8>, length: usize) -> Vec<u8> {
    let mut run: Option<Range<usize>> = None;
    let mut fresh = false;
    let mut in_place = true;

    for entry in &encoded {
        match (entry, &mut run) {
            (Encoded::Fresh(_, _), _) => fresh = true,
            (Encoded::Held(range, _), Some(run)) if !fresh && run.end == range.start => {
                run.end = range.end;
            }
            (Encoded::Held(range, _), None) if !fresh => run = Some(range.clone()),
            (Encoded::Held(_, _), _) => {
                in_place = false;
                break;
            }
        }
    }

    let mut wire = match run {
        _ if !in_place => {
            let mut wire = Vec::with_capacity(length);

            for entry in &encoded {
                match entry {
                    Encoded::Held(range, _) => wire.extend_from_slice(&previous[range.clone()]),
                    Encoded::Fresh(bytes, _) => wire.extend_from_slice(bytes),
                }
            }

            return wire;
        }
        Some(run) => {
            let mut wire = previous;
            wire.truncate(run.end);
            wire.drain(..run.start);
            wire
        }
        None => Vec::with_capacity(length),
    };

    for entry in encoded {
        if let Encoded::Fresh(bytes, _) = entry {
            wire.extend_from_slice(&bytes);
        }
    }

    wire
}

/// Earliest position needed to show `last` folded rows from the tail of `entries`.
/// Consecutive `Thought` and `Tool` entries fold into one progress row in the UI.
fn initial_start(entries: &[Entry], earlier: u64, last: u32) -> u64 {
    let limit = usize::try_from(last.clamp(1, MAX_TRANSCRIPT)).unwrap_or(usize::MAX);
    let mut rows = 0usize;
    let mut in_progress = false;
    let mut start = entries.len();

    for (index, entry) in entries.iter().enumerate().rev() {
        if entries.len() - index > INITIAL_ENTRIES {
            break;
        }

        let progress = matches!(entry.kind, EntryKind::Thought | EntryKind::Tool);

        if progress && in_progress {
            start = index;
            continue;
        }

        if rows == limit {
            break;
        }

        rows += 1;
        in_progress = progress;
        start = index;
    }

    earlier.saturating_add(u64::try_from(start).unwrap_or(u64::MAX))
}

fn next_update(
    snapshot: SessionSnapshot,
    mut previous: Option<Window>,
    request: &FollowSession,
) -> Result<(Window, SessionUpdate)> {
    let (mut window, unchanged) = retain(snapshot, previous.as_mut())?;
    let initial_start = initial_start(&window.entries, window.earlier, request.last);

    if let Some(prev) = previous {
        let held = prev.cursor.clone();
        let cached = prev.caches_held();

        // Retention loss, a source reset, or truncation before our window requires a fresh view.
        if window.earlier <= held.next && held.start < window.cursor.next {
            let start = held.start.max(window.earlier);
            let overlap_end = held.next.min(window.cursor.next);
            let first_diff = (start..overlap_end)
                .find(|&pos| {
                    !(if cached {
                        unchanged[window.index(pos)]
                    } else {
                        prev.hash_at(pos) == window.hash_at(pos)
                    })
                })
                .unwrap_or(overlap_end);

            if start == held.start && first_diff == held.next && first_diff == window.cursor.next {
                // The same entries encode the same bytes, so the checkpoint stands.
                window.cursor = held;
                window.known_from = prev.known_from.max(window.cursor.start);

                if request.patches {
                    let source = (window.source != prev.source).then(|| window.source.clone());
                    let patch = SessionPatch {
                        source,
                        cursor: window.cursor.clone(),
                        ops: Vec::new(),
                    };
                    return Ok((window, SessionUpdate::Patch(Box::new(patch))));
                }

                let delta = window.suffix(first_diff, false, false);
                return Ok((window, SessionUpdate::Delta(Box::new(delta))));
            }

            window.hold(start)?;

            if request.patches && first_diff >= prev.known_from {
                let mut ops = Vec::new();

                for pos in first_diff..window.cursor.next {
                    let same = pos < held.next
                        && if cached {
                            unchanged[window.index(pos)]
                        } else {
                            prev.hash_at(pos) == window.hash_at(pos)
                        };

                    if same {
                        continue;
                    }

                    let new_entry = &window.entries[window.index(pos)];

                    if pos < held.next
                        && cached
                        && let Some(old_entry) = prev.entry_at(pos)
                        && let Some(suffix) = append_text_suffix(old_entry, new_entry)
                    {
                        ops.push(EntryPatch::AppendText {
                            index: pos,
                            text: suffix.to_owned(),
                        });
                    } else {
                        ops.push(EntryPatch::Replace {
                            index: pos,
                            entry: new_entry.clone(),
                        });
                    }
                }

                if window.cursor.next < held.next {
                    ops.push(EntryPatch::Truncate {
                        next: window.cursor.next,
                    });
                }

                window.known_from = prev.known_from.max(window.cursor.start);
                let source = (window.source != prev.source).then(|| window.source.clone());
                let patch = SessionPatch {
                    source,
                    cursor: window.cursor.clone(),
                    ops,
                };
                return Ok((window, SessionUpdate::Patch(Box::new(patch))));
            }

            window.known_from = prev.known_from.min(first_diff).max(window.cursor.start);
            let delta = window.suffix(first_diff, false, request.patches);
            return Ok((window, SessionUpdate::Delta(Box::new(delta))));
        }
    } else if let Some(after) = &request.after {
        if validates(&window, after)? {
            window.hold(after.start)?;
            window.known_from = after.next;
            let update = window.suffix(after.next, true, request.patches);
            return Ok((window, SessionUpdate::Snapshot(Box::new(update))));
        }

        if request.patches
            && !after.prefix_fingerprint.is_empty()
            && after.prefix_next >= after.start
            && after.prefix_next < after.next
            && validates_range(
                &window,
                after.start,
                after.prefix_next,
                &after.prefix_fingerprint,
            )?
        {
            window.hold(after.start)?;
            window.known_from = after.prefix_next;
            let update = window.suffix(after.prefix_next, true, true);
            return Ok((window, SessionUpdate::Snapshot(Box::new(update))));
        }
    }

    window.hold(initial_start)?;
    window.known_from = window.cursor.start;
    let update = window.suffix(window.cursor.start, false, request.patches);
    Ok((window, SessionUpdate::Snapshot(Box::new(update))))
}

/// How one snapshot reads its session.
struct Read<'a> {
    /// The Claude Code session started outside Beep, when it is one.
    followed: Option<&'a mut Followed>,
    /// Whether its processes may have changed since the last read.
    stale: bool,
    /// How many of the newest entries to read.
    retained: usize,
    /// Where the reader's held entries start: earlier ones need not be read.
    from: Option<u64>,
}

async fn snapshot(
    agents: &Agents,
    request: &FollowSession,
    read: Read<'_>,
) -> Result<SessionSnapshot> {
    let Read {
        mut followed,
        stale,
        retained,
        from,
    } = read;
    let started = Instant::now();
    let source = match agents.get(&request.session) {
        Some(session) => {
            let info = session.info();

            if info.agent != request.agent.as_ref() {
                bail!("session belongs to another agent");
            }

            SessionSource::Owned(Box::new(info))
        }
        None => {
            let found = match followed.as_deref_mut() {
                Some(followed) => {
                    vec![
                        agents
                            .external()
                            .followed(followed, stale, agents.sessions.owned())
                            .await?,
                    ]
                }
                None => {
                    agents
                        .raw_external_of(request.agent.as_ref(), 1, Some(request.session.clone()))
                        .await?
                }
            };
            let mut found = found
                .into_iter()
                .find(|session| session.id == request.session)
                .context("session is no longer on the machine")?;
            agents.occupancy.settle_closed(std::slice::from_mut(&mut found));
            SessionSource::External(Box::new(found))
        }
    };
    transcript_stage("source", started.into_std());
    let started = Instant::now();
    // Source readers already bound their retained transcript and memoize disk parsing.
    // Read enough to validate the cached window, including mutable tools before its tail.
    let (entries, earlier) = agents
        .transcript_window(
            &request.session,
            retained,
            None,
            followed.map(|followed| (&*followed, from)),
            true,
        )
        .await?
        .context("session transcript is no longer on the machine")?;
    transcript_stage("transcript", started.into_std());
    let next = earlier
        .checked_add(u64::try_from(entries.len())?)
        .context("transcript position overflow")?;
    Ok(SessionSnapshot {
        source,
        entries,
        earlier,
        cursor: SessionCursor {
            start: earlier,
            next,
            fingerprint: String::new(),
            prefix_next: earlier,
            prefix_fingerprint: String::new(),
        },
        resumed: false,
        controls: false,
    })
}

async fn paths(
    agents: &Agents,
    request: &FollowSession,
    followed: Option<&Followed>,
) -> Result<Vec<PathBuf>> {
    if agents.get(&request.session).is_some() {
        return Ok(if request.agent == "codex" {
            agents.follow_roots(Agent::Codex)
        } else {
            Vec::new()
        });
    }

    let mut paths = Vec::new();

    let file = match followed {
        Some(followed) => Some(followed.path().to_owned()),
        None => agents.session_file(&request.session).await?,
    };

    if let Some(file) = file {
        if request.session.as_ref().starts_with("subagent:claude:") {
            paths.push(file.with_extension("meta.json"));
        }
        if matches!(request.agent.as_ref(), "flsh" | "opencode") {
            paths.push(crate::logs::found::wal_path(&file));
        }

        paths.push(file);
    }

    if request.agent == "claude" {
        paths.extend(agents.external().presence_path());
    } else if request.agent == "codex" {
        paths.extend(agents.follow_roots(Agent::Codex));
    }

    Ok(paths)
}

#[cfg(test)]
#[path = "../tests/agents_follow.rs"]
mod tests;
