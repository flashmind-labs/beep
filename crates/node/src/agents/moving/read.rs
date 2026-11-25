//! `beep-sessions/1`: a machine reads a session's transcript from the one
//! that ran it, as the bytes after what it already holds. A transcript only
//! grows, so a copy is brought up to date by asking for what follows it;
//! a hash of what the copy holds says whether it still is a start of the
//! source's.
//!
//! Sent as compact msgpack: a field is added only at the end with
//! `#[serde(default)]`.

use super::{
    Harness, files,
    progress::{Landing, Target},
    saved::{MOVED_AWAY, Outgoing, Previous, SentTree},
    worktree::{self, Work},
};
use crate::agents::{Agents, SavedSession, found::Agent};
use anyhow::{Context, Result, bail};
use beep_proto::{SessionId, watched};
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
    sync::{mpsc, watch},
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
/// before it answers.
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
        id: SessionId,
        from: u64,
        prefix_hash: Vec<u8>,
        /// Asked for a worktree's commit and uncommitted work by nodes from
        /// before worktree moves were removed. Sent false, ignored.
        #[serde(default)]
        worktree: bool,
        /// Keep the stream open after an `End` of a session that still
        /// runs, and send a second `End` once it does not. A source from
        /// before this closes the stream after the first. Added after
        /// `worktree`.
        #[serde(default)]
        follow: bool,
    },
    /// Asked by nodes from before background copies were removed; refused.
    /// Kept so the requests after it keep their place on the wire.
    List {},
    /// As `List`.
    Copy {
        agent: AgentSlug,
        id: SessionId,
        from: u64,
        prefix_hash: Vec<u8>,
    },
    /// Asks how the move of session `id` made at `at` stands, which the
    /// machine it moves to knows. Sent by the source.
    State { id: SessionId, at: u64 },
    /// Asked by nodes from before worktree moves were removed; refused.
    WorktreeBundle { id: SessionId, head: String },
    /// Asks for the commit of the project's checkout and the worktrees the
    /// session made, which move with it. Answered with `Checkout`.
    Checkout { id: SessionId },
    /// Asks for one of those worktrees, by its folder in the project, once
    /// the session ended: `Work`, then the commits main lacks as `Bundle`
    /// pieces when `bundle`, the uncommitted work as `Patch` pieces, and
    /// `BundleEnd`.
    Worktree {
        id: SessionId,
        path: String,
        bundle: bool,
        /// The checkout's commit the target matched, which the bundle
        /// leaves out.
        base: String,
    },
    /// Asks for what the folder of session `id`, a chat or one of its
    /// threads, holds, once the session ended: a tar as `Chunk` pieces,
    /// then `End`. The folders of the chat's threads that move themselves
    /// are left out.
    Files { id: SessionId },
    /// As `State`, kept open: the target sends a `State` each time the move
    /// stands differently, until it landed or failed. Sent by the source;
    /// a target from before this closes the stream without answering.
    Follow { id: SessionId, at: u64 },
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
    /// Were a worktree's commit and uncommitted work; never sent now. Kept
    /// so the replies after them keep their place on the wire.
    #[allow(dead_code)]
    Work {
        head: String,
    },
    #[allow(dead_code)]
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
    /// Was the answer to `List`; never sent now. Kept so the replies after
    /// it keep their place on the wire.
    #[allow(dead_code)]
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
        /// The target starts the session: past taking back, `landed` not
        /// yet. Added after `landed`.
        #[serde(default)]
        starting: bool,
    },
    /// Pieces of a Git bundle, answered only to `Worktree` (and, by nodes
    /// from before worktree moves were removed, `WorktreeBundle`).
    Bundle {
        data: Vec<u8>,
    },
    BundleEnd {},
    /// The answer to `Checkout`: the commit the project's checkout is at,
    /// none outside Git.
    Checkout {
        head: Option<String>,
        worktrees: Vec<MovingWorktree>,
        /// The checkout has uncommitted edits, which stay on the source.
        #[serde(default)]
        edits: bool,
        /// What the source saved of the session, which it keeps on the
        /// target. Added after `edits`.
        #[serde(default)]
        session: Option<Box<SavedSession>>,
        /// The bytes a chat's `Files` answer carries, for the move's
        /// progress; 0 for a project's session. Added after `session`.
        #[serde(default)]
        files: u64,
    },
}

/// A worktree that moves with its session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MovingWorktree {
    /// Its folder, relative to the project's checkout.
    pub(crate) path: String,
    pub(crate) branch: String,
    /// The commit it is at.
    pub(crate) head: String,
}

/// What `Checkout` answers.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Checkout {
    pub(crate) head: Option<String>,
    pub(crate) worktrees: Vec<MovingWorktree>,
    pub(crate) edits: bool,
    /// `None` from a source from before saved sessions.
    pub(crate) session: Option<SavedSession>,
    /// The bytes a chat's files take; 0 for a project or an older source.
    pub(crate) files: u64,
}

/// What a source keeps of a session that leaves, from its move record.
pub(crate) enum Leaving {
    /// The project's folder here and the worktrees the session made.
    Project {
        folder: PathBuf,
        worktrees: Vec<String>,
    },
    /// The folder of a chat or of one of its threads, and the folders in
    /// it that other moving sessions bring themselves.
    Chat {
        folder: PathBuf,
        apart: Vec<PathBuf>,
    },
}

/// One worktree as `Worktree` brought it.
#[derive(Debug)]
pub(crate) struct Fetched {
    pub(crate) work: Work,
    /// The commits main lacks, when asked for.
    pub(crate) bundle: Option<Vec<u8>>,
}

/// What a request is, for the check of who may make it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Asked {
    /// A target reads the transcript.
    Read,
    /// A source asks the target how the move stands.
    State(u64),
}

/// A session a node offered for a background copy, in the unused `Offers`
/// reply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Offer {
    pub(crate) agent: AgentSlug,
    pub(crate) id: SessionId,
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

type CurrentMove<'a> = dyn Fn(&SessionId) -> Option<Outgoing> + Send + Sync + 'a;

/// Checks the peer's access again while the move record reaches this machine.
pub(super) struct Access<'a> {
    pub(super) check: &'a (dyn Fn(Asked, &SessionId) -> bool + Send + Sync),
    pub(super) updates: Option<watch::Receiver<u64>>,
    /// The session's project and worktrees here, from its move record.
    pub(super) leaving: &'a (dyn Fn(&SessionId) -> Option<Leaving> + Send + Sync),
    pub(super) outgoing: Option<&'a CurrentMove<'a>>,
}

impl Access<'_> {
    async fn allows(&mut self, ask: Asked, id: &SessionId) -> bool {
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
) -> Result<()> {
    let refuse = |message: &str| Reply::Refused {
        message: message.to_owned(),
    };
    let Some(request) = rx.recv::<Request>().await? else {
        return Ok(());
    };
    let auth = match &request {
        Request::Read { id, .. }
        | Request::Checkout { id }
        | Request::Worktree { id, .. }
        | Request::Files { id } => Some((Asked::Read, id, "no move of that session to you")),
        Request::State { id, at } | Request::Follow { id, at } => {
            Some((Asked::State(*at), id, "no move of that session from you"))
        }
        Request::WorktreeBundle { .. } | Request::List {} | Request::Copy { .. } => None,
    };

    if let Some((asked, id, message)) = auth
        && !access.allows(asked, id).await
    {
        return tx.send(&refuse(message)).await;
    }

    if let Some((Asked::Read, id, message)) = auth
        && let Some(outgoing) = access.outgoing
        && !matches!(&request, Request::Read { .. })
    {
        let Some(generation) = outgoing(id) else {
            return tx.send(&refuse(message)).await;
        };
        if agents.moves.preceding(id, generation).await != Previous::Released {
            return tx.send(&refuse(MOVED_AWAY)).await;
        }
    }

    match request {
        Request::Read {
            agent,
            id,
            from,
            prefix_hash,
            worktree: _,
            follow,
        } => {
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
            let mut live = !agents.reopened() || agents.runs(&id) || agents.moves.holds(&id);
            // Whether the source says when it is done: not for a move still
            // settling from an earlier one, which the target asks again.
            let mut settling = false;
            if let Some(outgoing) = access.outgoing {
                let Some(generation) = outgoing(&id) else {
                    return tx.send(&refuse("no move of that session to you")).await;
                };
                match agents.moves.preceding(&id, generation).await {
                    Previous::Departed => return tx.send(&refuse(MOVED_AWAY)).await,
                    Previous::Settling => {
                        live = true;
                        settling = true;
                    }
                    Previous::Released => {
                        // A cancelled move may have reopened it during the read.
                        live |= agents.runs(&id) || agents.moves.holds(&id);
                    }
                }
            }

            match send_after(&mut tx, &found, from, &prefix_hash, live).await {
                Ok(()) => {
                    if follow && live && !settling {
                        until_done(agents, &id).await;
                        tx.send(&Reply::End {
                            size: 0,
                            live: false,
                        })
                        .await?;
                    }

                    tx.finish().await
                }
                Err(e) => {
                    let message = format!("{e:#}");
                    tx.send(&Reply::Refused { message }).await
                }
            }
        }
        Request::WorktreeBundle { .. } => {
            tx.send(&refuse("this machine no longer moves worktrees"))
                .await
        }
        Request::Checkout { id } => {
            let Some(leaving) = (access.leaving)(&id) else {
                return tx
                    .send(&refuse("this machine does not keep the project"))
                    .await;
            };
            let session = agents.saved(&id).map(Box::new);
            let reply = spawn_blocking(move || -> Result<Reply> {
                Ok(match leaving {
                    Leaving::Project { folder, worktrees } => Reply::Checkout {
                        head: worktree::head(&folder)?,
                        worktrees: worktree::moving(&folder, &worktrees),
                        edits: worktree::has_edits(&folder),
                        session,
                        files: 0,
                    },
                    // A chat's folder is no checkout, whatever it is inside.
                    Leaving::Chat { folder, apart } => Reply::Checkout {
                        head: None,
                        worktrees: Vec::new(),
                        edits: false,
                        session,
                        files: files::packed_size(&folder, &apart)?,
                    },
                })
            })
            .await?;

            match reply {
                Ok(reply) => tx.send(&reply).await?,
                Err(e) => tx.send(&refuse(&format!("{e:#}"))).await?,
            }

            tx.finish().await
        }
        Request::Worktree {
            id,
            path,
            bundle,
            base,
        } => {
            // While the sessions come back from a restart, or the source
            // ends this one again, the target asks again: no refusal, which
            // would end the move.
            if !agents.reopened() || agents.runs(&id) {
                return Ok(());
            }

            let Some(Leaving::Project { folder, worktrees }) = (access.leaving)(&id) else {
                return tx
                    .send(&refuse("this machine does not keep the project"))
                    .await;
            };

            let wanted = bundle.then_some(base);
            let sent = spawn_blocking(move || {
                send_worktree(&folder, &worktrees, &path, wanted.as_deref())
            })
            .await?;

            match sent {
                Ok((dir, work, bytes)) => {
                    let held = work.fingerprint();
                    agents.moves.sent(&id, SentTree { dir, held }).await;
                    tx.send(&Reply::Work { head: work.head }).await?;

                    for piece in bytes.iter().flat_map(|bytes| bytes.chunks(CHUNK)) {
                        tx.send(&Reply::Bundle {
                            data: piece.to_vec(),
                        })
                        .await?;
                    }

                    for piece in work.patch.chunks(CHUNK) {
                        tx.send(&Reply::Patch {
                            data: piece.to_vec(),
                        })
                        .await?;
                    }

                    tx.send(&Reply::BundleEnd {}).await?;
                    tx.finish().await
                }
                Err(e) => tx.send(&refuse(&format!("{e:#}"))).await,
            }
        }
        Request::Files { id } => {
            // As for a worktree: the target asks again.
            if !agents.reopened() || agents.runs(&id) {
                log::info!(
                    "session {id}: its files are asked for, but it still runs here or the sessions are not read yet; the target asks again"
                );
                return Ok(());
            }

            let Some(Leaving::Chat { folder, apart }) = (access.leaving)(&id) else {
                return tx.send(&refuse("that session is in no chat here")).await;
            };
            let (pieces, mut packed) = mpsc::channel(2);
            let packing = spawn_blocking(move || files::pack(&folder, &apart, Pieces(pieces)));
            let mut size = 0u64;

            while let Some(data) = packed.recv().await {
                size += data.len() as u64;
                tx.send(&Reply::Chunk { data }).await?;
            }

            match packing.await? {
                Ok(()) => {
                    tx.send(&Reply::End { size, live: false }).await?;
                    tx.finish().await
                }
                Err(e) => tx.send(&refuse(&format!("{e:#}"))).await,
            }
        }
        Request::List {} | Request::Copy { .. } => {
            tx.send(&refuse("this machine no longer makes background copies"))
                .await
        }
        Request::Follow { id, at } => {
            let Some(tracker) = agents.moves.tracker(&id, at) else {
                // The move is not here yet: it is asked again.
                tx.send(&state(&Target::Unknown)).await?;
                return tx.finish().await;
            };
            let mut landing = tracker.follow();
            let mut told = None;

            loop {
                let now = watched::read_and_update(&mut landing, Landing::target);

                if told.as_ref() != Some(&now) {
                    tx.send(&state(&now)).await?;
                }

                let over = matches!(now, Target::Landed | Target::Failed(_));
                told = Some(now);

                if over || landing.changed().await.is_err() {
                    return tx.finish().await;
                }
            }
        }
        Request::State { id, at } => {
            tx.send(&state(&agents.moves.target(&id, at))).await?;
            tx.finish().await
        }
    }
}

/// What a target says of a move, as `State` carries it.
fn state(target: &Target) -> Reply {
    let (taken, failed, landed, starting) = match target {
        Target::Unknown => (false, None, false, false),
        Target::Taken => (true, None, false, false),
        Target::Starting => (true, None, false, true),
        Target::Failed(reason) => (false, Some(reason.clone()), false, false),
        Target::Landed => (true, None, true, false),
    };
    Reply::State {
        taken,
        failed,
        landed,
        starting,
    }
}

/// Waits until session `id` no longer runs here and the source is done with it.
async fn until_done(agents: &Agents, id: &SessionId) {
    // Subscribed before the check, so a change between the two is not missed.
    let mut released = agents.moves.released();

    loop {
        agents.until_reopened().await;

        if !(agents.runs(id) || agents.moves.holds(id)) || released.changed().await.is_err() {
            return;
        }
    }
}

/// The worktree at `path` in the project's `folder`, if the session made it
/// (`worktrees`):
/// its folder, what it holds and, with a `base`, its commits that `base`
/// lacks. Blocks.
fn send_worktree(
    folder: &Path,
    worktrees: &[String],
    path: &str,
    base: Option<&str>,
) -> Result<(PathBuf, Work, Option<Vec<u8>>)> {
    let Some(tree) = worktree::moving(folder, worktrees)
        .into_iter()
        .find(|tree| tree.path == path)
    else {
        bail!("{path} is not a worktree of the session");
    };
    let dir = folder.join(&tree.path);
    let work = worktree::snapshot(&dir)?;
    let bytes = base
        .map(|base| worktree::bundle(&dir, &work.head, base))
        .transpose()?;
    Ok((dir, work, bytes))
}

/// Hands an archive, as it is written on a blocking thread, to the stream
/// in pieces a message carries.
struct Pieces(mpsc::Sender<Vec<u8>>);

impl std::io::Write for Pieces {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let piece = &buf[..buf.len().min(CHUNK)];
        self.0
            .blocking_send(piece.to_vec())
            .map_err(|_| std::io::Error::other("the reader left"))?;
        Ok(piece.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
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
    id: &SessionId,
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
) -> Result<()> {
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

    tx.send(&Reply::End { size, live }).await
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
    /// The source said on the stream that the session ended since: a read
    /// now brings the rest.
    pub(crate) ended: bool,
}

/// What `pull` asks for.
pub(crate) struct Ask<'a> {
    pub(crate) agent: AgentSlug,
    pub(crate) id: &'a SessionId,
    pub(crate) copy: &'a Path,
}

/// Brings the copy at `ask.copy` up to date from the stream of a source.
/// A copy that diverged is kept beside it as `<name>.diverged`.
#[cfg(test)]
pub(crate) async fn pull(tx: LinkTx, rx: LinkRx, ask: Ask<'_>) -> Result<Pulled> {
    pull_within(tx, rx, ask, QUIET_FOR).await
}

/// `pull`, asking a source that still runs the session to say when it ended
/// on this stream. `told` hears what came by the first `End`, before the wait.
pub(crate) async fn pull_following(
    tx: LinkTx,
    rx: LinkRx,
    ask: Ask<'_>,
    told: impl FnOnce(&Pulled),
) -> Result<Pulled> {
    pull_with(tx, rx, ask, QUIET_FOR, Some(told)).await
}

/// `pull`, failing when the source sends nothing for `quiet`.
#[cfg(test)]
pub(super) async fn pull_within(
    tx: LinkTx,
    rx: LinkRx,
    ask: Ask<'_>,
    quiet: Duration,
) -> Result<Pulled> {
    pull_with(tx, rx, ask, quiet, None::<fn(&Pulled)>).await
}

async fn pull_with(
    mut tx: LinkTx,
    mut rx: LinkRx,
    ask: Ask<'_>,
    quiet: Duration,
    told: Option<impl FnOnce(&Pulled)>,
) -> Result<Pulled> {
    let Ask { agent, id, copy } = ask;
    let (from, prefix_hash) = held(copy).await?;
    let request = Request::Read {
        agent,
        id: id.clone(),
        from,
        prefix_hash,
        worktree: false,
        follow: told.is_some(),
    };
    tx.send(&request).await?;
    let Some(Reply::Head {
        from: start,
        file_name,
    }) = next(&mut rx, quiet).await?
    else {
        bail!("the source ended without answering");
    };

    if !valid_file_name(&file_name) {
        return Err(Refused(format!("it named the transcript {file_name:?}")).into());
    }

    // A copy that is not a start of the source's is set aside for a fresh one.
    if from > 0 && start == 0 {
        let mut aside = copy.as_os_str().to_owned();
        aside.push(".diverged");
        fs::rename(copy, aside).await?;
    }

    let mut out = fs::OpenOptions::new();
    out.create(true).append(true);
    #[cfg(unix)]
    out.mode(0o600);
    let mut out = out.open(copy).await?;

    loop {
        match next(&mut rx, quiet).await? {
            Some(Reply::Chunk { data }) => {
                out.write_all(&data).await?;
            }
            Some(Reply::End { size, live }) => {
                out.sync_all().await?;
                let held = out.metadata().await?.len();

                if held != size {
                    bail!("the copy is {held} bytes, the source said {size}");
                }

                let mut pulled = Pulled {
                    size,
                    live,
                    file_name,
                    ended: false,
                };

                if let Some(told) = told
                    && live
                {
                    told(&pulled);

                    // A source that follows keeps the stream and ends it
                    // with a second `End`; an older one has closed it.
                    match rx.recv::<Reply>().await? {
                        Some(Reply::End { .. }) => pulled.ended = true,
                        Some(Reply::Refused { message }) => return Err(Refused(message).into()),
                        _ => {}
                    }
                }

                return Ok(pulled);
            }
            _ => bail!("the source stopped before the end"),
        }
    }
}

/// Asks a source for the commit of the project's checkout and the worktrees
/// that move with session `id`.
pub(crate) async fn checkout(mut tx: LinkTx, mut rx: LinkRx, id: &SessionId) -> Result<Checkout> {
    tx.send(&Request::Checkout { id: id.to_owned() }).await?;

    match next(&mut rx, QUIET_FOR).await? {
        Some(Reply::Checkout {
            head,
            worktrees,
            edits,
            session,
            files,
        }) => Ok(Checkout {
            head,
            worktrees,
            edits,
            session: session.map(|session| *session),
            files,
        }),
        // A node from before worktrees moved again cannot read the request.
        _ => Err(Refused("its beep-node is too old to move this session: update it".into()).into()),
    }
}

/// Asks a source for what the folder of session `id` holds, and writes the
/// archive to `into`, over what a try before left there, telling `came` the
/// bytes that came so far.
pub(crate) async fn files(
    mut tx: LinkTx,
    mut rx: LinkRx,
    id: &SessionId,
    into: &Path,
    mut came: impl FnMut(u64),
) -> Result<()> {
    tx.send(&Request::Files { id: id.to_owned() }).await?;
    let mut out = File::create(into).await?;
    let mut held = 0u64;

    loop {
        match next(&mut rx, QUIET_FOR).await? {
            Some(Reply::Chunk { data }) => {
                held += data.len() as u64;

                if held > files::MAX_FILES {
                    return Err(Refused("the chat's files are too large to move".into()).into());
                }

                out.write_all(&data).await?;
                came(held);
            }
            Some(Reply::End { size, .. }) if size == held => return Ok(out.sync_all().await?),
            Some(Reply::End { size, .. }) => bail!("{held} bytes came, the source said {size}"),
            // A source that still ends the session answers nothing: asked again.
            _ => bail!("the source stopped before the end of the files"),
        }
    }
}

/// What a target asks of one worktree.
pub(crate) struct WorktreeAsk<'a> {
    pub(crate) id: &'a SessionId,
    /// Its folder in the project, as `Checkout` named it.
    pub(crate) path: &'a str,
    /// The checkout's commit both machines are at.
    pub(crate) base: &'a str,
    /// Its commits are not here yet.
    pub(crate) bundle: bool,
}

/// Asks a source for one of a session's worktrees.
pub(crate) async fn worktree(
    mut tx: LinkTx,
    mut rx: LinkRx,
    ask: &WorktreeAsk<'_>,
) -> Result<Fetched> {
    let bundle = ask.bundle;
    tx.send(&Request::Worktree {
        id: ask.id.clone(),
        path: ask.path.to_owned(),
        bundle,
        base: ask.base.to_owned(),
    })
    .await?;
    let Some(Reply::Work { head }) = next(&mut rx, QUIET_FOR).await? else {
        bail!("the source ended without sending the worktree");
    };
    let mut bytes = Vec::new();
    let mut patch = Vec::new();

    loop {
        match next(&mut rx, QUIET_FOR).await? {
            Some(Reply::Bundle { data }) => grow(&mut bytes, &data, worktree::MAX_BUNDLE)?,
            Some(Reply::Patch { data }) => grow(&mut patch, &data, worktree::MAX_PATCH)?,
            Some(Reply::BundleEnd {}) => break,
            _ => bail!("the source stopped before the end of the worktree"),
        }
    }

    Ok(Fetched {
        work: Work { head, patch },
        bundle: bundle.then_some(bytes),
    })
}

/// Adds `data` to `into`, refusing more than `cap` bytes in all.
fn grow(into: &mut Vec<u8>, data: &[u8], cap: usize) -> Result<()> {
    if into.len() + data.len() > cap {
        bail!("the worktree is too large to move");
    }

    into.extend_from_slice(data);
    Ok(())
}

/// The source's next message within `quiet`, turning `Refused` into `Err`.
async fn next(rx: &mut LinkRx, quiet: Duration) -> Result<Option<Reply>> {
    match timeout(quiet, rx.recv::<Reply>())
        .await
        .with_context(|| format!("the source sent nothing for {}s", quiet.as_secs()))??
    {
        Some(Reply::Refused { message }) => Err(Refused(message).into()),
        other => Ok(other),
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
