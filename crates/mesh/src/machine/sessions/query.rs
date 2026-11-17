use super::{MODELS_TIMEOUT, Reading, Sessions, ask, hinted, once};
use crate::{
    MeshError,
    machine::{said, unexpected},
};
use beep_proto::SessionId;
use beep_proto::node::agent::{
    Entry, ExternalAsk, ExternalSession, HistoryCursor, LiveStatus, ModelList, SessionFolder,
    SessionInfo, SessionPage, SessionsReply, SessionsRequest, Steer,
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
    session: &SessionId,
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
    session: &SessionId,
    last: u32,
    thoughts: bool,
    before: Option<u64>,
) -> Result<(Vec<Entry>, u32), MeshError> {
    let request = SessionsRequest::Transcript {
        session: session.clone(),
        last,
        thoughts,
        before,
    };

    match once(node, request).await? {
        SessionsReply::Transcript { entries, earlier } => Ok((entries, earlier)),
        SessionsReply::Error { message } => Err(said(message)),
        other => Err(unexpected(&other)),
    }
}

/// Sessions of `agent` started outside Beep, newest first: at most `limit`,
/// or only `only`. A list is followed; one session is looked up once.
pub async fn external<S: Sessions + ?Sized>(
    node: &S,
    agent: &str,
    limit: u32,
    only: Option<SessionId>,
) -> Result<Vec<ExternalSession>, MeshError> {
    let reading = match only {
        Some(_) => Reading::Once,
        None => Reading::Followed,
    };
    let request = SessionsRequest::External {
        agent: agent.to_owned(),
        limit,
        session: only,
    };
    let reply = node.sessions(request, reading, super::READ_TIMEOUT).await;
    external_reply(hinted(reply, "list them")?)
}

/// The sessions `asks` started outside Beep, each looked up once; `None`
/// from a node from before the question, which the caller asks one by one.
pub async fn external_named<S: Sessions + ?Sized>(
    node: &S,
    asks: Vec<ExternalAsk>,
) -> Result<Option<Vec<ExternalSession>>, MeshError> {
    match once(node, SessionsRequest::ExternalNamed { sessions: asks }).await {
        Ok(reply) => external_reply(reply).map(Some),
        // A node from before it cannot read the request and closes the stream.
        Err(MeshError::Link(_)) => Ok(None),
        Err(e) => Err(e),
    }
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

/// One metadata page, or `None` when the node does not support pagination.
pub async fn in_folders_page<S: Sessions + ?Sized>(
    node: &S,
    folders: Vec<String>,
    limit: u32,
    before: Option<HistoryCursor>,
) -> Result<Option<SessionPage>, MeshError> {
    match once(
        node,
        SessionsRequest::InFoldersPage {
            folders,
            limit,
            before,
        },
    )
    .await
    {
        Ok(SessionsReply::SessionPage { page }) => Ok(Some(page)),
        Err(MeshError::Link(_)) => Ok(None),
        Ok(SessionsReply::Error { message }) => Err(said(message)),
        Ok(other) => Err(unexpected(&other)),
        Err(error) => Err(error),
    }
}

/// How Claude Code session `session`, started outside Beep, does now, after
/// queuing `steer` for its next hook when given.
pub async fn live<S: Sessions + ?Sized>(
    node: &S,
    session: &SessionId,
    steer: Option<Steer>,
) -> Result<LiveStatus, MeshError> {
    let session = session.clone();
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
        once(node, SessionsRequest::Folders { limit }).await,
        "find them",
    )? {
        SessionsReply::Folders { folders } => Ok(folders),
        SessionsReply::Error { message } => Err(said(message)),
        other => Err(unexpected(&other)),
    }
}

/// The models `agent` can run on the node. Link failures remain errors so
/// callers can retry rather than treating them as a fixed model.
pub async fn models<S: Sessions + ?Sized>(node: &S, agent: &str) -> Result<ModelList, MeshError> {
    let request = SessionsRequest::Models {
        agent: agent.to_owned(),
    };

    match node.sessions(request, Reading::Once, MODELS_TIMEOUT).await {
        Ok(SessionsReply::Models { models }) => Ok(models),
        Ok(SessionsReply::Error { message }) => Err(said(message)),
        Ok(other) => Err(unexpected(&other)),
        Err(e) => Err(e),
    }
}
