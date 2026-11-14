use super::{NodeWork, READ_TIMEOUT, Reading, Sessions, once};
use crate::{
    MeshError,
    machine::{said, unexpected},
};
use beep_proto::node::agent::{MoveProgress, SessionsReply, SessionsRequest, Worktree};

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

/// The worktrees of the repository holding `folder` on the node; none when
/// it is not in one. With `session`, each says whether that session made it.
pub async fn worktrees<S: Sessions + ?Sized>(
    node: &S,
    folder: &str,
    session: Option<&str>,
) -> Result<NodeWork<Vec<Worktree>>, MeshError> {
    let request = SessionsRequest::Worktrees {
        folder: folder.to_owned(),
        session: session.map(str::to_owned),
    };

    match once(node, request).await? {
        SessionsReply::Worktrees { worktrees } => Ok(NodeWork::Done(worktrees)),
        SessionsReply::Error { message } => Ok(NodeWork::Refused(message)),
        other => Err(unexpected(&other)),
    }
}
