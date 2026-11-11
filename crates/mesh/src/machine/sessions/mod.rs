//! `agents/1`: one request, one reply, about all of a node's sessions.
//! `Sessions` is anything that can send one; the typed requests below are
//! written once for every caller.

mod actions;
mod files;
mod moves;
mod query;

pub use actions::{
    commands, delete_session, diff, harnesses, install_adapters, relocate_session, review, reviews,
    start_review, update_harness,
};
pub use files::{
    custom_agents, edit_files, folder, image, media, present, read_file, thumb, tree, upload_file,
    write_file,
};
pub use moves::{
    MoveState, NodeCopies, WorktreeMove, continue_copy, copies, discard_copy, move_progress,
    worktree_move, worktrees,
};
pub use query::{
    external, external_named, found_folders, in_folders, list, list_all, live, models, transcript,
    transcript_before,
};

use super::{said, timed_out};
use crate::{MeshError, PeerClient};
use beep_proto::node::agent::{SESSIONS_PROTOCOL, SessionsReply, SessionsRequest};
use std::{future::Future, time::Duration};
use tracing::Instrument;

/// How long an ordinary read of the node's sessions may take.
const READ_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a picture may take: a whole one is a few MiB, which a link
/// through a busy relay carries in far more than `READ_TIMEOUT`.
const IMAGE_TIMEOUT: Duration = Duration::from_secs(60);
/// How long a node has to list models: it may start the agent first.
const MODELS_TIMEOUT: Duration = Duration::from_secs(60);
/// How long a node has to read a diff: git may walk a large checkout.
const DIFF_TIMEOUT: Duration = Duration::from_secs(60);
/// How long a node may take to report its harnesses: it asks npm for versions.
const REPORT_TIMEOUT: Duration = Duration::from_secs(45);
/// How long an install or update may take on the node, and a little more.
const UPDATE_TIMEOUT: Duration = Duration::from_secs(11 * 60);
/// How long a node may take to start a reviewer: it counts the agents under
/// the parent on every machine and waits for the agent's id.
const REVIEW_TIMEOUT: Duration = Duration::from_secs(60);

/// How a read is kept. A view the app follows is served from the live query
/// that owns it, which the node reads again on every matching change; a read
/// asked once goes to the node as one call and leaves no live query behind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reading {
    Followed,
    Once,
}

/// Sends one `agents/1` request and gives the node's reply within `limit`.
pub trait Sessions {
    fn sessions(
        &self,
        request: SessionsRequest,
        reading: Reading,
        limit: Duration,
    ) -> impl Future<Output = Result<SessionsReply, MeshError>> + Send;
}

impl Sessions for PeerClient {
    async fn sessions(
        &self,
        request: SessionsRequest,
        reading: Reading,
        limit: Duration,
    ) -> Result<SessionsReply, MeshError> {
        let read = beep_proto::node::events::Read::Sessions(Box::new(request.clone()));

        if reading == Reading::Followed
            && read.topic().is_some()
            && let Some(value) = tokio::time::timeout(limit, self.snapshot(read))
                .await
                .map_err(|e| timed_out(e, "the session subscription did not answer in time"))?
        {
            return match value? {
                beep_proto::node::events::Value::Sessions(reply) => Ok(*reply),
                _ => Err(said("the session subscription returned another resource")),
            };
        }

        let operation = match &request {
            SessionsRequest::List {} | SessionsRequest::ListAll {} => "list",
            SessionsRequest::Transcript { .. } => "transcript",
            _ => "other",
        };
        self.call_within(limit, SESSIONS_PROTOCOL, &request)
            .instrument(tracing::info_span!("agent_request", operation))
            .await
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

/// An ordinary read of a view the app follows.
async fn ask<S: Sessions + ?Sized>(
    node: &S,
    request: SessionsRequest,
) -> Result<SessionsReply, MeshError> {
    node.sessions(request, Reading::Followed, READ_TIMEOUT)
        .await
}

/// An ordinary read asked once, or a change.
async fn once<S: Sessions + ?Sized>(
    node: &S,
    request: SessionsRequest,
) -> Result<SessionsReply, MeshError> {
    node.sessions(request, Reading::Once, READ_TIMEOUT).await
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
