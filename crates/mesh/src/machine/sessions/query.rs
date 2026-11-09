use super::{MODELS_TIMEOUT, Sessions, ask, hinted};
use crate::{
    MeshError,
    machine::{said, unexpected},
};
use beep_proto::node::agent::{
    Entry, ExternalSession, LiveStatus, ModelList, SessionFolder, SessionInfo, SessionsReply,
    SessionsRequest, Steer,
};

/// Every agent session on the node.
pub async fn list<S: Sessions + ?Sized>(node: &S) -> Result<Vec<SessionInfo>, MeshError> {
    match ask(node, SessionsRequest::List {}).await? {
        SessionsReply::Sessions { sessions } => Ok(sessions),
        SessionsReply::Error { message } => Err(said(message)),
        other => Err(unexpected(&other)),
    }
}

/// `list` with the running reviewers that have a parent. A node from
/// before `ListAll` closes the stream, and gives `list` instead.
pub async fn list_all<S: Sessions + ?Sized>(node: &S) -> Result<Vec<SessionInfo>, MeshError> {
    match ask(node, SessionsRequest::ListAll {}).await {
        Ok(SessionsReply::Sessions { sessions }) => Ok(sessions),
        Ok(SessionsReply::Error { message }) => Err(said(message)),
        Ok(other) => Err(unexpected(&other)),
        Err(MeshError::Link(_)) => list(node).await,
        Err(e) => Err(e),
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
