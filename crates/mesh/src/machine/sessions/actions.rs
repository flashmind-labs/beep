use super::{
    DIFF_TIMEOUT, NodeWork, READ_TIMEOUT, REPORT_TIMEOUT, REVIEW_TIMEOUT, Sessions, UPDATE_TIMEOUT,
    ask, hinted,
};
use crate::{
    MeshError,
    machine::{said, unexpected},
};
use beep_proto::{
    WorkDiff,
    node::{
        agent::{AgentCommand, DiffAsk, ReviewOrder, ReviewRun, SessionsReply, SessionsRequest},
        harness::HarnessReport,
    },
};

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

/// Starts a temporary reviewer on the node as `order` says; it refuses one
/// over the limits, or an agent it does not have.
pub async fn start_review<S: Sessions + ?Sized>(
    node: &S,
    order: ReviewOrder,
) -> Result<NodeWork<ReviewRun>, MeshError> {
    let request = SessionsRequest::StartReview { order };

    match hinted(node.sessions(request, REVIEW_TIMEOUT).await, "review")? {
        SessionsReply::Review { run } => Ok(NodeWork::Done(run)),
        SessionsReply::Error { message } => Ok(NodeWork::Refused(message)),
        other => Err(unexpected(&other)),
    }
}

/// How the review `id` on the node stands.
pub async fn review<S: Sessions + ?Sized>(
    node: &S,
    id: &str,
) -> Result<NodeWork<ReviewRun>, MeshError> {
    let request = SessionsRequest::Review { id: id.to_owned() };

    match ask(node, request).await? {
        SessionsReply::Review { run } => Ok(NodeWork::Done(run)),
        SessionsReply::Error { message } => Ok(NodeWork::Refused(message)),
        other => Err(unexpected(&other)),
    }
}

/// The reviews of the session `session`'s work on the node, oldest first;
/// none from a node from before reviews.
pub async fn reviews<S: Sessions + ?Sized>(
    node: &S,
    session: &str,
) -> Result<Vec<ReviewRun>, MeshError> {
    let request = SessionsRequest::Reviews {
        session: session.to_owned(),
    };

    match ask(node, request).await? {
        SessionsReply::Reviews { runs } => Ok(runs),
        SessionsReply::Error { .. } => Ok(Vec::new()),
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

/// Installs the ACP adapters the node's agents lack now; `false` from a
/// node too old to.
pub async fn install_adapters<S: Sessions + ?Sized>(node: &S) -> Result<bool, MeshError> {
    match node
        .sessions(SessionsRequest::InstallAdapters {}, READ_TIMEOUT)
        .await
    {
        Ok(SessionsReply::AdaptersInstalling {}) => Ok(true),
        Ok(SessionsReply::Error { message }) if message.starts_with("this node does not serve") => {
            Ok(false)
        }
        Ok(SessionsReply::Error { message }) => Err(said(message)),
        Ok(other) => Err(unexpected(&other)),
        // A node from before it cannot read the request and closes the stream.
        Err(MeshError::Link(_)) => Ok(false),
        Err(e) => Err(e),
    }
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

/// Moves a closed session to another working folder.
pub async fn relocate_session<S: Sessions + ?Sized>(
    node: &S,
    agent: &str,
    session: &str,
    folder: &str,
) -> Result<NodeWork<()>, MeshError> {
    match hinted(
        ask(
            node,
            SessionsRequest::RelocateSession {
                agent: agent.to_owned(),
                session: session.to_owned(),
                folder: folder.to_owned(),
            },
        )
        .await,
        "move sessions between projects",
    )? {
        SessionsReply::Deleted {} => Ok(NodeWork::Done(())),
        SessionsReply::Error { message } => Ok(NodeWork::Refused(message)),
        other => Err(unexpected(&other)),
    }
}
