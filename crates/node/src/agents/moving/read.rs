//! `beep-sessions/1`: a machine reads a session's transcript from the one
//! that ran it, as the bytes after what it already holds. A transcript only
//! grows, so a copy is brought up to date by asking for what follows it;
//! a hash of what the copy holds says whether it still is a start of the
//! source's.
//!
//! Sent as compact msgpack: a field is added only at the end with
//! `#[serde(default)]`.

use super::{
    Harness,
    offers::{Project, offers},
    progress::Target,
    worktree::{self, MAX_BUNDLE, MAX_PATCH, Work, snapshot},
};
use crate::agents::{Agents, found::Agent};
use anyhow::{Context, Result, bail};
use beep_proto::{
    AgentSlug,
    node::{LinkRx, LinkTx},
};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::{
    io::SeekFrom,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{
    fs::{self, File},
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
    sync::watch,
    task::spawn_blocking,
    time::timeout,
};

pub const ALPN: &[u8] = b"beep-sessions/1";

/// The most bytes of a transcript one stream carries per message, under
/// the link's frame limit.
const CHUNK: usize = 256 * 1024;
/// The largest transcript served. A bigger one is not moved.
pub(super) const MAX_TRANSCRIPT: u64 = 1024 * 1024 * 1024;
/// How long a read waits for the source's next message before it gives up,
/// so a stalled stream fails and is tried again. The source hashes the copy
/// and may snapshot a worktree before it answers.
const QUIET_FOR: Duration = Duration::from_secs(60);
const AUTHORIZE_FOR: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub(crate) enum Request {
    /// Sends session `id` of `agent` after its first `from` bytes, when
    /// `prefix_hash` (blake3) is the hash of the requester's first `from`
    /// bytes, else all of it. Ignored when `from` is 0.
    Read {
        agent: AgentSlug,
        id: String,
        from: u64,
        prefix_hash: Vec<u8>,
        /// The session works in a worktree: after the last bytes, when the
        /// session does not run, send its commit and uncommitted work.
        #[serde(default)]
        worktree: bool,
    },
    /// What this machine offers for background copies: its sessions of the
    /// mesh's projects, active in the last 30 days.
    List {},
    /// As `Read`, for a background copy: any mesh member may ask, for a
    /// session `List` offers. Nothing of a worktree is sent.
    Copy {
        agent: AgentSlug,
        id: String,
        from: u64,
        prefix_hash: Vec<u8>,
    },
    /// Asks how the move of session `id` made at `at` stands, which the
    /// machine it moves to knows. Sent by the source.
    State { id: String, at: u64 },
    /// The commits of a moving session's worktree. Only this request receives
    /// `Bundle` and `BundleEnd`, so older transcript readers keep working.
    WorktreeBundle { id: String, head: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub(crate) enum Reply {
    /// Sent first: where the bytes that follow start (0 when the
    /// requester's copy is not a start of this one) and the name the
    /// source keeps the file under.
    Head {
        from: u64,
        file_name: String,
    },
    Chunk {
        data: Vec<u8>,
    },
    /// Sent before `End` of a worktree session that does not run: the commit
    /// its worktree is at. A patch of the uncommitted work follows, in
    /// `Patch` pieces.
    Work {
        head: String,
    },
    Patch {
        data: Vec<u8>,
    },
    /// Last: the transcript's size now, and whether the session still
    /// runs, so it may grow.
    End {
        size: u64,
        live: bool,
    },
    Refused {
        message: String,
    },
    /// The answer to `List`.
    Offers {
        offers: Vec<Offer>,
    },
    /// The answer to `State`.
    State {
        /// The target can take the session.
        taken: bool,
        /// Why the target gave up, when it did.
        failed: Option<String>,
        /// The session runs on the target.
        landed: bool,
    },
    /// Pieces of a Git bundle, answered only to `WorktreeBundle`.
    Bundle {
        data: Vec<u8>,
    },
    BundleEnd {},
}

/// What a request is, for the check of who may make it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Asked {
    /// A target reads the transcript.
    Read,
    /// A source asks the target how the move stands.
    State,
    /// Any mesh member lists what this machine offers, or copies one.
    Background,
}

/// A session this machine offers other machines a background copy of.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Offer {
    pub(crate) agent: AgentSlug,
    pub(crate) id: String,
    /// The mesh project it ran in.
    pub(crate) project: String,
    /// The folder it ran in, relative to the project's checkout here.
    pub(crate) path: String,
    /// Unix seconds.
    pub(crate) last_activity: u64,
    /// The transcript's size and its blake3 hash.
    pub(crate) size: u64,
    pub(crate) hash: Vec<u8>,
}

/// Checks the peer's access again while the move record reaches this machine.
pub(super) struct Access<'a> {
    pub(super) check: &'a (dyn Fn(Asked, &str) -> bool + Send + Sync),
    pub(super) updates: Option<watch::Receiver<u64>>,
}

impl Access<'_> {
    async fn allows(&mut self, ask: Asked, id: &str) -> bool {
        if (self.check)(ask, id) {
            return true;
        }
        let Some(updates) = self.updates.as_mut() else {
            return false;
        };
        timeout(AUTHORIZE_FOR, async {
            while updates.changed().await.is_ok() {
                if (self.check)(ask, id) {
                    return true;
                }
            }
            false
        })
        .await
        .unwrap_or(false)
    }
}

/// The source turned a read away, for good: asking again changes nothing.
#[derive(Debug)]
pub(crate) struct Refused(pub(crate) String);

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the source refused: {}", self.0)
    }
}

impl std::error::Error for Refused {}

/// A bare transcript name: no folder, and the `.jsonl` every harness's
/// transcript has. The name comes from another machine, so it must not
/// lead out of the store.
pub(super) fn valid_file_name(name: &str) -> bool {
    let bare = Path::new(name)
        .file_name()
        .is_some_and(|only| only == std::ffi::OsStr::new(name));
    bare && name.len() <= 255
        && name.len() > ".jsonl".len()
        && name.ends_with(".jsonl")
        && !name.starts_with('.')
        && !name.contains(['/', '\\', '\0'])
}

/// Answers one request on a stream; `me` is what the mesh calls this
/// machine.
pub(super) async fn serve(
    mut tx: LinkTx,
    mut rx: LinkRx,
    agents: &Agents,
    me: &str,
    mut access: Access<'_>,
    projects: &[Project],
) -> Result<()> {
    let refuse = |message: &str| Reply::Refused {
        message: message.to_owned(),
    };
    match rx.recv::<Request>().await? {
        Some(Request::Read {
            agent,
            id,
            from,
            prefix_hash,
            worktree,
        }) => {
            if !access.allows(Asked::Read, &id).await {
                return tx.send(&refuse("no move of that session to you")).await;
            }
            if let Some(why) = agents.moves.refusal(&id) {
                return tx.send(&refuse(&why)).await;
            }
            let found = match locate(agents, me, &agent, &id).await {
                Ok(found) => found,
                Err(message) => return tx.send(&Reply::Refused { message }).await,
            };
            // Whether it runs is read before the file, so bytes written
            // between the two are never missed by a reader that sees it
            // ended. A session that may still be coming back from a
            // restart, or that the source is still finishing with, counts
            // as running.
            // A worktree asked for that the source has not registered yet
            // (its task has not started) is not read as ended: it would
            // come without its commit and work.
            let unregistered = worktree && agents.moves.worktree_of(&id).is_none();
            let live =
                !agents.reopened() || agents.runs(&id) || agents.moves.holds(&id) || unregistered;
            let work = match worktree.then(|| agents.moves.worktree_of(&id)).flatten() {
                Some(dir) if !live => Some(dir),
                _ => None,
            };
            // What only the snapshot could refuse (a repository of its own
            // among the new files, changes too large) is refused on the first
            // read, while the session still runs: the source stops nothing
            // for a move that cannot be taken.
            if live
                && worktree
                && let Some(dir) = agents.moves.worktree_of(&id)
                && agents.moves.first_preview(&id)
            {
                // The snapshot itself, bounded at `MAX_PATCH` and thrown
                // away: whatever it cannot read, the final read could not
                // either.
                match spawn_blocking(move || snapshot(&dir).map(|_| ())).await {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => {
                        let message = format!("{e:#}");
                        return tx.send(&Reply::Refused { message }).await;
                    }
                    Err(e) => {
                        let message = format!("{e}");
                        return tx.send(&Reply::Refused { message }).await;
                    }
                }
            }
            match send_after(&mut tx, &found, from, &prefix_hash, live, work).await {
                Ok(sent) => {
                    if let Some(sent) = sent {
                        agents.moves.set_sent(&id, sent);
                    }
                    tx.finish().await
                }
                Err(e) => {
                    let message = format!("{e:#}");
                    tx.send(&Reply::Refused { message }).await
                }
            }
        }
        Some(Request::WorktreeBundle { id, head }) => {
            if !access.allows(Asked::Read, &id).await {
                return tx.send(&refuse("no move of that session to you")).await;
            }
            if let Some(why) = agents.moves.refusal(&id) {
                return tx.send(&refuse(&why)).await;
            }
            let Some(dir) = agents.moves.worktree_of(&id) else {
                return tx.send(&refuse("the session has no moving worktree")).await;
            };
            let bundled = spawn_blocking(move || worktree::bundle(&dir, &head)).await?;
            let bytes = match bundled {
                Ok(bytes) => bytes,
                Err(e) => return tx.send(&refuse(&format!("{e:#}"))).await,
            };
            for chunk in bytes.chunks(CHUNK) {
                tx.send(&Reply::Bundle {
                    data: chunk.to_vec(),
                })
                .await?;
            }
            tx.send(&Reply::BundleEnd {}).await?;
            tx.finish().await
        }
        Some(Request::List {}) => {
            if !access.allows(Asked::Background, "").await {
                return tx.send(&refuse("not in the mesh")).await;
            }
            let offers = offers(agents, projects).await;
            tx.send(&Reply::Offers { offers }).await?;
            tx.finish().await
        }
        Some(Request::Copy {
            agent,
            id,
            from,
            prefix_hash,
        }) => {
            if !access.allows(Asked::Background, &id).await {
                return tx.send(&refuse("not in the mesh")).await;
            }
            // Only what `List` offers: a session of a mesh project.
            let offered = offers(agents, projects)
                .await
                .iter()
                .any(|offer| offer.id == id && offer.agent == agent);
            if !offered {
                return tx.send(&refuse("that session is not offered")).await;
            }
            let found = match locate(agents, me, &agent, &id).await {
                Ok(found) => found,
                Err(message) => return tx.send(&Reply::Refused { message }).await,
            };
            let live = !agents.reopened() || agents.runs(&id) || agents.moves.holds(&id);
            match send_after(&mut tx, &found, from, &prefix_hash, live, None).await {
                Ok(_) => tx.finish().await,
                Err(e) => {
                    let message = format!("{e:#}");
                    tx.send(&Reply::Refused { message }).await
                }
            }
        }
        Some(Request::State { id, at }) => {
            if !access.allows(Asked::State, &id).await {
                return tx.send(&refuse("no move of that session from you")).await;
            }
            let (taken, failed, landed) = match agents.moves.target(&id, at) {
                Target::Unknown => (false, None, false),
                Target::Taken => (true, None, false),
                Target::Failed(reason) => (false, Some(reason), false),
                Target::Landed => (true, None, true),
            };
            tx.send(&Reply::State {
                taken,
                failed,
                landed,
            })
            .await?;
            tx.finish().await
        }
        None => Ok(()),
    }
}

/// A transcript to serve.
struct Found {
    path: PathBuf,
    file_name: String,
}

async fn locate(
    agents: &Agents,
    me: &str,
    agent: &AgentSlug,
    id: &str,
) -> std::result::Result<Found, String> {
    let movable = agent
        .as_ref()
        .parse::<Agent>()
        .ok()
        .and_then(|agent| Harness::try_from(agent).ok())
        .ok_or_else(|| format!("{agent} sessions cannot move"))?;
    let path = match agents.transcript_file(movable, id).await {
        Some(path) => path,
        None if movable == Harness::Flsh && agents.flsh_in_database(id).await => {
            return Err(super::flsh::outdated(me));
        }
        None => return Err(format!("this machine has no {agent} session {id}")),
    };
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .ok_or_else(|| "the transcript has no file name".to_owned())?;
    Ok(Found { path, file_name })
}

async fn send_after(
    tx: &mut LinkTx,
    found: &Found,
    from: u64,
    prefix_hash: &[u8],
    live: bool,
    worktree: Option<PathBuf>,
) -> Result<Option<String>> {
    let mut file = File::open(&found.path).await?;
    let size = file.metadata().await?.len();
    if size > MAX_TRANSCRIPT {
        bail!("the transcript is too large to move");
    }
    let start = if from > 0 && from <= size && prefix_of(&mut file, from).await? == prefix_hash {
        from
    } else {
        0
    };
    file.seek(SeekFrom::Start(start)).await?;
    tx.send(&Reply::Head {
        from: start,
        file_name: found.file_name.clone(),
    })
    .await?;
    // Only what existed when the read began: a live session may append
    // meanwhile, and the End says how far this went.
    let mut left = size - start;
    let mut buf = vec![0; CHUNK];
    while left > 0 {
        let want = usize::try_from(left).map_or(CHUNK, |left| left.min(CHUNK));
        let got = file.read(&mut buf[..want]).await?;
        if got == 0 {
            bail!("the transcript got shorter");
        }
        left -= got as u64;
        tx.send(&Reply::Chunk {
            data: buf[..got].to_vec(),
        })
        .await?;
    }
    let mut sent = None;
    if let Some(dir) = worktree {
        let work = spawn_blocking(move || snapshot(&dir)).await??;
        sent = Some(work.fingerprint());
        tx.send(&Reply::Work { head: work.head }).await?;
        for piece in work.patch.chunks(CHUNK) {
            tx.send(&Reply::Patch {
                data: piece.to_vec(),
            })
            .await?;
        }
    }
    tx.send(&Reply::End { size, live }).await?;
    // What a worktree held when it was sent, for the source to know later
    // that it still holds that and nothing else.
    Ok(sent)
}

/// The blake3 hash of the first `len` bytes of `file`.
async fn prefix_of(file: &mut File, len: u64) -> Result<Vec<u8>> {
    file.seek(SeekFrom::Start(0)).await?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0; CHUNK];
    let mut left = len;
    while left > 0 {
        let want = usize::try_from(left).map_or(CHUNK, |left| left.min(CHUNK));
        let got = file.read(&mut buf[..want]).await?;
        if got == 0 {
            break;
        }
        hasher.update(&buf[..got]);
        left -= got as u64;
    }
    Ok(hasher.finalize().as_bytes().to_vec())
}

/// What one `pull` brought.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Pulled {
    /// The copy's size now, which is the source's size at the read.
    pub(crate) size: u64,
    /// The session still ran at the source, so more may follow.
    pub(crate) live: bool,
    /// The name the source keeps the file under.
    pub(crate) file_name: String,
    /// The copy was not a start of the source's: it was set aside, and
    /// this is a fresh one.
    pub(crate) restarted: bool,
    /// The worktree's commit and uncommitted work, when asked for and the
    /// session did not run.
    pub(crate) work: Option<Work>,
}

/// What `pull` asks for.
pub(crate) struct Ask<'a> {
    pub(crate) agent: AgentSlug,
    pub(crate) id: &'a str,
    pub(crate) copy: &'a Path,
    pub(crate) kind: Kind,
}

/// Whose copy it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// The target of a move reads the session; `worktree` asks for the
    /// worktree's commit and uncommitted work as well.
    Move { worktree: bool },
    /// A background copy, at most `rate` bytes a second.
    Background { rate: u64 },
}

/// Brings the copy at `ask.copy` up to date from the stream of a source.
/// A copy that diverged is kept beside it as `<name>.diverged`.
pub(crate) async fn pull(tx: LinkTx, rx: LinkRx, ask: Ask<'_>) -> Result<Pulled> {
    pull_within(tx, rx, ask, QUIET_FOR).await
}

/// `pull`, failing when the source sends nothing for `quiet`.
pub(super) async fn pull_within(
    mut tx: LinkTx,
    mut rx: LinkRx,
    ask: Ask<'_>,
    quiet: Duration,
) -> Result<Pulled> {
    let Ask {
        agent,
        id,
        copy,
        kind,
    } = ask;
    let (from, prefix_hash) = held(copy).await?;
    let request = match kind {
        Kind::Move { worktree } => Request::Read {
            agent,
            id: id.to_owned(),
            from,
            prefix_hash,
            worktree,
        },
        Kind::Background { .. } => Request::Copy {
            agent,
            id: id.to_owned(),
            from,
            prefix_hash,
        },
    };
    tx.send(&request).await?;
    let (start, file_name) = match next(&mut rx, quiet).await? {
        Some(Reply::Head { from, file_name }) => (from, file_name),
        Some(Reply::Refused { message }) => return Err(Refused(message).into()),
        _ => bail!("the source ended without answering"),
    };
    if !valid_file_name(&file_name) {
        return Err(Refused(format!("it named the transcript {file_name:?}")).into());
    }
    let restarted = from > 0 && start == 0;
    if restarted {
        let mut aside = copy.as_os_str().to_owned();
        aside.push(".diverged");
        fs::rename(copy, aside).await?;
    }
    let mut out = fs::OpenOptions::new();
    out.create(true).append(true);
    #[cfg(unix)]
    out.mode(0o600);
    let mut out = out.open(copy).await?;
    let mut work: Option<Work> = None;
    loop {
        match next(&mut rx, quiet).await? {
            Some(Reply::Chunk { data }) => {
                out.write_all(&data).await?;
                if let Kind::Background { rate } = kind {
                    // Receiving slowly slows the sender through the
                    // connection's flow control: a background copy never
                    // takes the link.
                    let secs = data.len() as f64 / rate.max(1) as f64;
                    tokio::time::sleep(std::time::Duration::from_secs_f64(secs)).await;
                }
            }
            Some(Reply::Work { head }) => {
                work = Some(Work {
                    head,
                    patch: Vec::new(),
                });
            }
            Some(Reply::Patch { data }) => {
                let Some(work) = work.as_mut() else {
                    bail!("the source sent a patch before its commit");
                };
                if work.patch.len() + data.len() > MAX_PATCH {
                    return Err(Refused("the uncommitted changes are too large".into()).into());
                }
                work.patch.extend_from_slice(&data);
            }
            Some(Reply::End { size, live }) => {
                out.sync_all().await?;
                let held = out.metadata().await?.len();
                if held != size {
                    bail!("the copy is {held} bytes, the source said {size}");
                }
                return Ok(Pulled {
                    size,
                    live,
                    file_name,
                    restarted,
                    work,
                });
            }
            Some(Reply::Refused { message }) => return Err(Refused(message).into()),
            _ => bail!("the source stopped before the end"),
        }
    }
}

/// Reads commits separately from the transcript, without extending existing replies.
pub(super) async fn pull_bundle(
    mut tx: LinkTx,
    mut rx: LinkRx,
    id: &str,
    head: &str,
) -> Result<Vec<u8>> {
    tx.send(&Request::WorktreeBundle {
        id: id.to_owned(),
        head: head.to_owned(),
    })
    .await?;
    let mut bytes = Vec::new();
    loop {
        match next(&mut rx, QUIET_FOR).await? {
            Some(Reply::Bundle { data }) => {
                if data.len() > MAX_BUNDLE.saturating_sub(bytes.len()) {
                    bail!("the worktree's commit history is too large to move");
                }
                bytes.extend_from_slice(&data);
            }
            Some(Reply::BundleEnd {}) => return Ok(bytes),
            Some(Reply::Refused { message }) => return Err(Refused(message).into()),
            _ => {
                bail!("the source could not send the worktree's commits; update its node and retry")
            }
        }
    }
}

/// The source's next message, if it sends one within `quiet`.
async fn next(rx: &mut LinkRx, quiet: Duration) -> Result<Option<Reply>> {
    timeout(quiet, rx.recv::<Reply>())
        .await
        .with_context(|| format!("the source sent nothing for {}s", quiet.as_secs()))?
}

/// The size of `copy` and the hash of what it holds; empty when it does
/// not exist.
async fn held(copy: &Path) -> Result<(u64, Vec<u8>)> {
    let mut file = match File::open(copy).await {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((0, Vec::new())),
        Err(e) => return Err(e).context("reading the copy"),
    };
    let len = file.metadata().await?.len();
    Ok((len, prefix_of(&mut file, len).await?))
}
