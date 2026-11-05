//! `agents/1`: one request, one reply, about all of a node's sessions.
//! `Sessions` is anything that can send one; the typed requests below are
//! written once for every caller.

use super::{said, unexpected};
use crate::{MeshError, PeerClient};
use beep_proto::{
    CustomAgent, ListWorkTree, WorkDiff, WorkFolder, WorkTree,
    node::{
        agent::{
            AgentCommand, CopyView, DiffAsk, Entry, ExternalSession, LiveStatus, ModelList,
            MoveProgress, SESSIONS_PROTOCOL, SessionFolder, SessionInfo, SessionsReply,
            SessionsRequest, Steer, Worktree,
        },
        harness::HarnessReport,
    },
};
use std::{future::Future, time::Duration};

/// How long an ordinary read of the node's sessions may take.
pub const READ_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a node has to list models: it may start the agent first.
pub const MODELS_TIMEOUT: Duration = Duration::from_secs(60);
/// How long a node has to read a diff: git may walk a large checkout.
pub const DIFF_TIMEOUT: Duration = Duration::from_secs(60);
/// How long a node may take to report its harnesses: it asks npm for versions.
pub const REPORT_TIMEOUT: Duration = Duration::from_secs(45);
/// How long an install or update may take on the node, and a little more.
pub const UPDATE_TIMEOUT: Duration = Duration::from_secs(11 * 60);

/// Sends one `agents/1` request and gives the node's reply within `limit`.
pub trait Sessions {
    fn sessions(
        &self,
        request: SessionsRequest,
        limit: Duration,
    ) -> impl Future<Output = Result<SessionsReply, MeshError>> + Send;
}

impl Sessions for PeerClient {
    async fn sessions(
        &self,
        request: SessionsRequest,
        limit: Duration,
    ) -> Result<SessionsReply, MeshError> {
        self.call_within(limit, SESSIONS_PROTOCOL, &request).await
    }
}

/// A failure of an older node, which cannot read a request, with a hint
/// that updating it may help.
fn hinted<T>(reply: Result<T, MeshError>, to: &str) -> Result<T, MeshError> {
    reply.map_err(|e| match e {
        MeshError::Link(e) => MeshError::Link(e.context(format!(
            "the node may need an update {}{to}",
            if to.starts_with("for") { "" } else { "to " }
        ))),
        other => other,
    })
}

/// An ordinary read.
async fn ask<S: Sessions + ?Sized>(
    node: &S,
    request: SessionsRequest,
) -> Result<SessionsReply, MeshError> {
    node.sessions(request, READ_TIMEOUT).await
}

/// A picture of a session's transcript, as the node gave it.
pub struct NodeImage {
    pub mime: String,
    pub data: Vec<u8>,
}

/// What a node answered when asked for part of a file.
pub enum NodeFile {
    Chunk {
        size: u64,
        mime: String,
        data: Vec<u8>,
    },
    /// There is no file at that path.
    Missing,
    /// The node would not read it, saying why.
    Refused(String),
}

/// What a node answered when asked to work in a session's folder.
pub enum NodeWork<T> {
    Done(T),
    /// The node would not, saying why.
    Refused(String),
}

/// A read of part of a file in a session's folder.
pub struct WorkRead<'a> {
    pub root: &'a str,
    pub path: &'a str,
    pub offset: u64,
    pub len: u32,
}

/// One part of a file written into a session's folder.
pub struct WorkWrite {
    pub root: String,
    pub path: String,
    pub offset: u64,
    pub size: u64,
    pub data: Vec<u8>,
}

/// Every agent session on the node.
pub async fn list<S: Sessions + ?Sized>(node: &S) -> Result<Vec<SessionInfo>, MeshError> {
    match ask(node, SessionsRequest::List {}).await? {
        SessionsReply::Sessions { sessions } => Ok(sessions),
        SessionsReply::Error { message } => Err(said(message)),
        other => Err(unexpected(&other)),
    }
}

/// The last `last` transcript entries of `session` and how many came before them.
pub async fn transcript<S: Sessions + ?Sized>(
    node: &S,
    session: &str,
    last: u32,
    thoughts: bool,
) -> Result<(Vec<Entry>, u32), MeshError> {
    transcript_before(node, session, last, thoughts, None).await
}

/// As `transcript`, the `last` entries before entry `before` when given. A
/// node from before `before` answers with the last entries: an answer whose
/// `earlier` plus entries (thoughts kept) does not end at `before` is that.
pub async fn transcript_before<S: Sessions + ?Sized>(
    node: &S,
    session: &str,
    last: u32,
    thoughts: bool,
    before: Option<u64>,
) -> Result<(Vec<Entry>, u32), MeshError> {
    let request = SessionsRequest::Transcript {
        session: session.to_owned(),
        last,
        thoughts,
        before,
    };
    match ask(node, request).await? {
        SessionsReply::Transcript { entries, earlier } => Ok((entries, earlier)),
        SessionsReply::Error { message } => Err(said(message)),
        other => Err(unexpected(&other)),
    }
}

/// Sessions of `agent` started outside Beep, newest first: at most `limit`, or only `only`.
pub async fn external<S: Sessions + ?Sized>(
    node: &S,
    agent: &str,
    limit: u32,
    only: Option<String>,
) -> Result<Vec<ExternalSession>, MeshError> {
    let request = SessionsRequest::External {
        agent: agent.to_owned(),
        limit,
        session: only,
    };
    external_reply(hinted(ask(node, request).await, "list them")?)
}

/// The sessions coding agents ran in `folders` or below, newest first.
pub async fn in_folders<S: Sessions + ?Sized>(
    node: &S,
    folders: Vec<String>,
    limit: u32,
) -> Result<Vec<ExternalSession>, MeshError> {
    external_reply(hinted(
        ask(node, SessionsRequest::InFolders { folders, limit }).await,
        "list them",
    )?)
}

fn external_reply(reply: SessionsReply) -> Result<Vec<ExternalSession>, MeshError> {
    match reply {
        SessionsReply::External { sessions } => Ok(sessions),
        SessionsReply::Error { message } => Err(said(message)),
        other => Err(unexpected(&other)),
    }
}

/// How Claude Code session `session`, started outside Beep, does now, after
/// queuing `steer` for its next hook when given.
pub async fn live<S: Sessions + ?Sized>(
    node: &S,
    session: &str,
    steer: Option<Steer>,
) -> Result<LiveStatus, MeshError> {
    let session = session.to_owned();
    let request = match steer {
        Some(steer) => SessionsRequest::Steer { session, steer },
        None => SessionsRequest::Live { session },
    };
    match hinted(ask(node, request).await, "for live status")? {
        SessionsReply::Live { status } => Ok(*status),
        SessionsReply::Error { message } => Err(said(message)),
        other => Err(unexpected(&other)),
    }
}

/// The git checkouts coding agents worked in on the node, newest first.
pub async fn found_folders<S: Sessions + ?Sized>(
    node: &S,
    limit: u32,
) -> Result<Vec<SessionFolder>, MeshError> {
    match hinted(
        ask(node, SessionsRequest::Folders { limit }).await,
        "find them",
    )? {
        SessionsReply::Folders { folders } => Ok(folders),
        SessionsReply::Error { message } => Err(said(message)),
        other => Err(unexpected(&other)),
    }
}

/// The models `agent` can run on the node; `None` from a node too old to say.
pub async fn models<S: Sessions + ?Sized>(
    node: &S,
    agent: &str,
) -> Result<Option<ModelList>, MeshError> {
    let request = SessionsRequest::Models {
        agent: agent.to_owned(),
    };
    match node.sessions(request, MODELS_TIMEOUT).await {
        Ok(SessionsReply::Models { models }) => Ok(Some(models)),
        Ok(SessionsReply::Error { message }) => Err(said(message)),
        Ok(other) => Err(unexpected(&other)),
        // A node that cannot say, such as one from before models, closes the stream.
        Err(MeshError::Link(e)) => {
            tracing::debug!("node did not list models; agent={agent} error={e:#}");
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

/// The picture `key` of `session`'s transcript; `None` when the node no longer has it.
pub async fn image<S: Sessions + ?Sized>(
    node: &S,
    session: &str,
    key: &str,
) -> Result<Option<NodeImage>, MeshError> {
    let request = SessionsRequest::Image {
        session: session.to_owned(),
        key: key.to_owned(),
    };
    match ask(node, request).await? {
        SessionsReply::Image { mime, data } => Ok(Some(NodeImage { mime, data })),
        SessionsReply::Error { .. } => Ok(None),
        other => Err(unexpected(&other)),
    }
}

/// Part of the media file `path` on the node, from `offset`.
pub async fn media<S: Sessions + ?Sized>(
    node: &S,
    path: &str,
    offset: u64,
) -> Result<NodeFile, MeshError> {
    let request = SessionsRequest::File {
        path: path.to_owned(),
        offset,
    };
    file_reply(ask(node, request).await?)
}

/// Part of a file inside a session folder on the node.
pub async fn read_file<S: Sessions + ?Sized>(
    node: &S,
    read: WorkRead<'_>,
) -> Result<NodeFile, MeshError> {
    let request = SessionsRequest::ReadFile {
        root: read.root.to_owned(),
        path: read.path.to_owned(),
        offset: read.offset,
        len: read.len,
    };
    file_reply(ask(node, request).await?)
}

fn file_reply(reply: SessionsReply) -> Result<NodeFile, MeshError> {
    match reply {
        SessionsReply::File { size, mime, data } => Ok(NodeFile::Chunk { size, mime, data }),
        SessionsReply::NoFile {} => Ok(NodeFile::Missing),
        SessionsReply::Error { message } => Ok(NodeFile::Refused(message)),
        other => Err(unexpected(&other)),
    }
}

/// The folder `path` inside the session folder `root` on the node.
pub async fn folder<S: Sessions + ?Sized>(
    node: &S,
    root: &str,
    path: &str,
    ignored: bool,
) -> Result<NodeWork<WorkFolder>, MeshError> {
    let request = SessionsRequest::Folder {
        root: root.to_owned(),
        path: path.to_owned(),
        ignored,
        depth: 0,
    };
    match ask(node, request).await? {
        SessionsReply::Folder { folder } => Ok(NodeWork::Done(folder)),
        SessionsReply::Error { message } => Ok(NodeWork::Refused(message)),
        other => Err(unexpected(&other)),
    }
}

/// The folder `ask.path` inside `ask.root` and those under it, `ask.depth`
/// levels in all; `ask.path` alone from a node from before trees.
pub async fn tree<S: Sessions + ?Sized>(
    node: &S,
    list: &ListWorkTree,
) -> Result<NodeWork<WorkTree>, MeshError> {
    let request = SessionsRequest::Folder {
        root: list.root.clone(),
        path: list.path.clone(),
        ignored: list.ignored,
        depth: list.depth.max(1),
    };
    match ask(node, request).await? {
        SessionsReply::Tree { tree } => Ok(NodeWork::Done(tree)),
        SessionsReply::Folder { folder } => Ok(NodeWork::Done(WorkTree {
            folders: vec![folder],
            truncated: false,
        })),
        SessionsReply::Error { message } => Ok(NodeWork::Refused(message)),
        other => Err(unexpected(&other)),
    }
}

/// Writes one part of a file into a session folder on the node; gives the path it went to.
pub async fn write_file<S: Sessions + ?Sized>(
    node: &S,
    write: WorkWrite,
) -> Result<NodeWork<String>, MeshError> {
    let request = SessionsRequest::WriteFile {
        root: write.root,
        path: write.path,
        offset: write.offset,
        size: write.size,
        data: write.data,
    };
    match ask(node, request).await? {
        SessionsReply::Written { path } => Ok(NodeWork::Done(path)),
        SessionsReply::Error { message } => Ok(NodeWork::Refused(message)),
        other => Err(unexpected(&other)),
    }
}

/// The custom agents the harnesses define for `folder` on the node and in its home.
pub async fn custom_agents<S: Sessions + ?Sized>(
    node: &S,
    folder: &str,
) -> Result<NodeWork<Vec<CustomAgent>>, MeshError> {
    let request = SessionsRequest::CustomAgents {
        folder: folder.to_owned(),
    };
    match ask(node, request).await? {
        SessionsReply::CustomAgents { agents } => Ok(NodeWork::Done(agents)),
        SessionsReply::Error { message } => Ok(NodeWork::Refused(message)),
        other => Err(unexpected(&other)),
    }
}

/// What a node says of a session's move to it.
pub enum MoveState {
    Known(MoveProgress),
    /// The node knows of no such move.
    Unknown,
    /// The node is from before moves: it cannot take a session.
    NeedsUpdate,
}

/// How the move of `session` to the node stands, as the node says.
pub async fn move_progress<S: Sessions + ?Sized>(
    node: &S,
    session: &str,
) -> Result<MoveState, MeshError> {
    let request = SessionsRequest::MoveProgress {
        session: session.to_owned(),
    };
    match node.sessions(request, READ_TIMEOUT).await {
        Ok(SessionsReply::Moving {
            progress: Some(progress),
        }) => Ok(MoveState::Known(progress)),
        Ok(SessionsReply::Moving { progress: None }) => Ok(MoveState::Unknown),
        // A node from before moves says it does not serve them...
        Ok(SessionsReply::Error { message }) if message.starts_with("this node does not serve") => {
            Ok(MoveState::NeedsUpdate)
        }
        Ok(SessionsReply::Error { message }) => Err(said(message)),
        Ok(other) => Err(unexpected(&other)),
        // ...or closes the stream.
        Err(MeshError::Link(e)) => {
            tracing::debug!("node did not answer a move request; error={e:#}");
            Ok(MoveState::NeedsUpdate)
        }
        Err(e) => Err(e),
    }
}

/// Whether a node can take a session working in a worktree on `branch` of
/// `project`.
pub enum WorktreeMove {
    Takes,
    /// Its Syncthing is set not to sync the worktree's folder.
    Ignored,
    /// The node is from before worktree moves.
    NeedsUpdate,
}

/// Asks the node whether it can take a worktree session of `project`.
pub async fn worktree_move<S: Sessions + ?Sized>(
    node: &S,
    project: &str,
    branch: &str,
) -> Result<WorktreeMove, MeshError> {
    let request = SessionsRequest::MoveCheck {
        project: project.to_owned(),
        branch: branch.to_owned(),
    };
    match node.sessions(request, READ_TIMEOUT).await {
        Ok(SessionsReply::MoveChecked { ignored: false }) => Ok(WorktreeMove::Takes),
        Ok(SessionsReply::MoveChecked { ignored: true }) => Ok(WorktreeMove::Ignored),
        Ok(SessionsReply::Error { message }) => Err(said(message)),
        Ok(other) => Err(unexpected(&other)),
        // A node from before it closes the stream.
        Err(MeshError::Link(e)) => {
            tracing::debug!("node did not answer a move check; error={e:#}");
            Ok(WorktreeMove::NeedsUpdate)
        }
        Err(e) => Err(e),
    }
}

/// What a node says of the copies of other machines' sessions it keeps.
pub enum NodeCopies {
    Listed(Vec<CopyView>),
    /// The node is from before background copies.
    NeedsUpdate,
}

/// The copies the node keeps, and the versions that lost to another machine's.
pub async fn copies<S: Sessions + ?Sized>(node: &S) -> Result<NodeCopies, MeshError> {
    match node
        .sessions(SessionsRequest::Copies {}, READ_TIMEOUT)
        .await
    {
        Ok(SessionsReply::Copies { copies }) => Ok(NodeCopies::Listed(copies)),
        Ok(SessionsReply::Error { message }) => Err(said(message)),
        Ok(other) => Err(unexpected(&other)),
        // A node from before it closes the stream.
        Err(MeshError::Link(e)) => {
            tracing::debug!("node did not answer a copies request; error={e:#}");
            Ok(NodeCopies::NeedsUpdate)
        }
        Err(e) => Err(e),
    }
}

/// Runs the node's copy of a session, or its version set aside at `lost`.
pub async fn continue_copy<S: Sessions + ?Sized>(
    node: &S,
    agent: &str,
    session: &str,
    lost: Option<u64>,
) -> Result<NodeWork<()>, MeshError> {
    let request = SessionsRequest::ContinueCopy {
        agent: agent.to_owned(),
        session: session.to_owned(),
        lost,
    };
    match node.sessions(request, UPDATE_TIMEOUT).await? {
        SessionsReply::CopyDone {} => Ok(NodeWork::Done(())),
        SessionsReply::Error { message } => Ok(NodeWork::Refused(message)),
        other => Err(unexpected(&other)),
    }
}

/// Throws away the node's copy of a session, or its version set aside at `lost`.
pub async fn discard_copy<S: Sessions + ?Sized>(
    node: &S,
    agent: &str,
    session: &str,
    lost: Option<u64>,
) -> Result<(), MeshError> {
    let request = SessionsRequest::DiscardCopy {
        agent: agent.to_owned(),
        session: session.to_owned(),
        lost,
    };
    match ask(node, request).await? {
        SessionsReply::CopyDone {} => Ok(()),
        SessionsReply::Error { message } => Err(said(message)),
        other => Err(unexpected(&other)),
    }
}

/// The worktrees of the repository holding `folder` on the node; none when
/// it is not in one.
pub async fn worktrees<S: Sessions + ?Sized>(
    node: &S,
    folder: &str,
) -> Result<NodeWork<Vec<Worktree>>, MeshError> {
    let request = SessionsRequest::Worktrees {
        folder: folder.to_owned(),
    };
    match ask(node, request).await? {
        SessionsReply::Worktrees { worktrees } => Ok(NodeWork::Done(worktrees)),
        SessionsReply::Error { message } => Ok(NodeWork::Refused(message)),
        other => Err(unexpected(&other)),
    }
}

/// The commands and skills the running session `session` offers on the
/// node; none when it is not running or the agent reports none.
pub async fn commands<S: Sessions + ?Sized>(
    node: &S,
    session: &str,
) -> Result<NodeWork<Vec<AgentCommand>>, MeshError> {
    let request = SessionsRequest::Commands {
        session: session.to_owned(),
    };
    match ask(node, request).await? {
        SessionsReply::Commands { commands } => Ok(NodeWork::Done(commands)),
        SessionsReply::Error { message } => Ok(NodeWork::Refused(message)),
        other => Err(unexpected(&other)),
    }
}

/// Deletes the ended session `session` of `agent` from the node; it refuses
/// one that runs, and a harness it cannot edit.
pub async fn delete_session<S: Sessions + ?Sized>(
    node: &S,
    agent: &str,
    session: &str,
) -> Result<NodeWork<()>, MeshError> {
    let request = SessionsRequest::DeleteSession {
        agent: agent.to_owned(),
        session: session.to_owned(),
    };
    match hinted(ask(node, request).await, "delete sessions")? {
        SessionsReply::Deleted {} => Ok(NodeWork::Done(())),
        SessionsReply::Error { message } => Ok(NodeWork::Refused(message)),
        other => Err(unexpected(&other)),
    }
}

/// What changed in the checkout `ask.root` on the node.
pub async fn diff<S: Sessions + ?Sized>(
    node: &S,
    ask_for: DiffAsk,
) -> Result<NodeWork<WorkDiff>, MeshError> {
    match node
        .sessions(SessionsRequest::Diff { ask: ask_for }, DIFF_TIMEOUT)
        .await?
    {
        SessionsReply::Diff { diff } => Ok(NodeWork::Done(diff)),
        SessionsReply::Error { message } => Ok(NodeWork::Refused(message)),
        other => Err(unexpected(&other)),
    }
}

/// What the node finds of each harness; `None` from a node too old to say.
pub async fn harnesses<S: Sessions + ?Sized>(
    node: &S,
    refresh: bool,
) -> Result<Option<HarnessReport>, MeshError> {
    harness_reply(
        node.sessions(SessionsRequest::Harnesses { refresh }, REPORT_TIMEOUT)
            .await,
    )
}

/// Installs or updates `agent` on the node; `None` from a node too old to.
pub async fn update_harness<S: Sessions + ?Sized>(
    node: &S,
    agent: &str,
) -> Result<Option<HarnessReport>, MeshError> {
    let request = SessionsRequest::UpdateHarness {
        agent: agent.to_owned(),
    };
    harness_reply(node.sessions(request, UPDATE_TIMEOUT).await)
}

fn harness_reply(
    reply: Result<SessionsReply, MeshError>,
) -> Result<Option<HarnessReport>, MeshError> {
    match reply {
        Ok(SessionsReply::Harnesses { report }) => Ok(Some(report)),
        // A node from before harnesses says it does not serve them.
        Ok(SessionsReply::Error { message }) if message.starts_with("this node does not serve") => {
            Ok(None)
        }
        Ok(SessionsReply::Error { message }) => Err(said(message)),
        Ok(other) => Err(unexpected(&other)),
        // A node from before harnesses closes the stream.
        Err(MeshError::Link(e)) => {
            tracing::debug!("node did not report its harnesses; error={e:#}");
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;

    /// A node whose link is down.
    struct Down;

    impl Sessions for Down {
        async fn sessions(
            &self,
            _: SessionsRequest,
            _: Duration,
        ) -> Result<SessionsReply, MeshError> {
            Err(MeshError::Link(anyhow!("the node closed the stream")))
        }
    }

    #[tokio::test]
    async fn a_node_that_cannot_say_has_no_models_or_harnesses() {
        assert!(models(&Down, "claude").await.expect("no error").is_none());
        assert!(harnesses(&Down, false).await.expect("no error").is_none());
        assert!(
            update_harness(&Down, "claude")
                .await
                .expect("no error")
                .is_none()
        );
    }

    #[tokio::test]
    async fn an_older_node_is_told_to_update() {
        let said = |e: MeshError| format!("{e:#}");
        let e = external(&Down, "claude", 1, None).await.expect_err("down");
        assert!(said(e).contains("the node may need an update to list them"));
        let e = live(&Down, "s", None).await.expect_err("down");
        assert!(said(e).contains("the node may need an update for live status"));
        let e = found_folders(&Down, 1).await.expect_err("down");
        assert!(said(e).contains("the node may need an update to find them"));
        let e = in_folders(&Down, Vec::new(), 1).await.expect_err("down");
        assert!(said(e).contains("the node may need an update to list them"));
    }
}
