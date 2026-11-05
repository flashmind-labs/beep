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
    worktree::{MAX_PATCH, Work, preflight, snapshot},
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
};
use tokio::{
    fs::{self, File},
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
    task::spawn_blocking,
};

pub(crate) const ALPN: &[u8] = b"beep-sessions/1";

/// The most bytes of a transcript one stream carries per message, under
/// the link's frame limit.
const CHUNK: usize = 256 * 1024;
/// The largest transcript served. A bigger one is not moved.
pub(super) const MAX_TRANSCRIPT: u64 = 1024 * 1024 * 1024;

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

/// Whether the peer may make a request of this kind about session `id`.
pub(super) type Allow<'a> = &'a (dyn Fn(Asked, &str) -> bool + Send + Sync);

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

/// Answers one request on a stream.
pub(super) async fn serve(
    mut tx: LinkTx,
    mut rx: LinkRx,
    agents: &Agents,
    allow: Allow<'_>,
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
            if !allow(Asked::Read, &id) {
                return tx.send(&refuse("no move of that session to you")).await;
            }
            if let Some(why) = agents.moves.refusal(&id) {
                return tx.send(&refuse(&why)).await;
            }
            let found = match locate(agents, &agent, &id).await {
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
            {
                match spawn_blocking(move || preflight(&dir)).await {
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
        Some(Request::List {}) => {
            if !allow(Asked::Background, "") {
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
            if !allow(Asked::Background, &id) {
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
            let found = match locate(agents, &agent, &id).await {
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
            if !allow(Asked::State, &id) {
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
    agent: &AgentSlug,
    id: &str,
) -> std::result::Result<Found, String> {
    let movable = Agent::from_name(agent.as_str())
        .and_then(Harness::of)
        .ok_or_else(|| format!("{agent} sessions cannot move"))?;
    let path = agents
        .transcript_file(movable, id)
        .await
        .ok_or_else(|| format!("this machine has no {agent} session {id}"))?;
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
pub(crate) async fn pull(mut tx: LinkTx, mut rx: LinkRx, ask: Ask<'_>) -> Result<Pulled> {
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
    let (start, file_name) = match rx.recv::<Reply>().await? {
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
        match rx.recv::<Reply>().await? {
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
