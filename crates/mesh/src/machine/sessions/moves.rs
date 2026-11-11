use super::{NodeWork, READ_TIMEOUT, Reading, Sessions, UPDATE_TIMEOUT, once};
use crate::{
    MeshError,
    machine::{said, unexpected},
};
use beep_proto::node::agent::{CopyView, MoveProgress, SessionsReply, SessionsRequest, Worktree};

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

    match node
        .sessions(request, Reading::Followed, READ_TIMEOUT)
        .await
    {
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

    match node.sessions(request, Reading::Once, READ_TIMEOUT).await {
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
        .sessions(SessionsRequest::Copies {}, Reading::Followed, READ_TIMEOUT)
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

    match node
        .sessions(request, Reading::Once, UPDATE_TIMEOUT)
        .await?
    {
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

    match once(node, request).await? {
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

    match once(node, request).await? {
        SessionsReply::Worktrees { worktrees } => Ok(NodeWork::Done(worktrees)),
        SessionsReply::Error { message } => Ok(NodeWork::Refused(message)),
        other => Err(unexpected(&other)),
    }
}
