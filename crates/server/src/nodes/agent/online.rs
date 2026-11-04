//! The coding-agent requests a server makes of one online node.

use super::{
    super::{Follower, Online, RECONNECT_WAIT, retry::Unsent},
    DIFF_TIMEOUT, Failed, Failure, MODELS_TIMEOUT, NodeImage, ORDERS, Opening, Retry, follow,
    wake::Reporter,
};
use crate::{
    api::transport::READ_TIMEOUT,
    nodes::{AgentLink, NodeFile, NodeWork, SessionRefused, StartOn, Watched, WorkRead, WorkWrite},
    protocol::{
        CustomAgent, ListWorkTree, WorkDiff, WorkFolder, WorkTree,
        node::{
            LinkRx, LinkTx,
            agent::{
                self, DiffAsk, Entry, Event, ExternalSession, LiveStatus, ModelList, Request,
                SESSIONS_PROTOCOL, SessionFolder, SessionInfo, SessionsReply, SessionsRequest,
                Steer,
            },
        },
    },
};
use anyhow::{Context, Result, bail};
use beep_mesh::{MeshError, PeerClient};
use std::{
    sync::{Arc, atomic::AtomicUsize},
    time::Duration,
};
use tokio::{
    sync::{mpsc, watch},
    time::{Instant, timeout},
};
use tokio_util::task::AbortOnDropHandle;

impl Online {
    /// Makes `session` report to `owner`'s chat when no call waits for it.
    pub fn report_to(&self, session: &str, owner: Follower) {
        self.agents.set_owner(session, owner);
    }

    /// Starts a session of `agent` in `cwd` on `prompt` and follows it.
    pub async fn start_agent(
        &self,
        agent: &str,
        cwd: Option<String>,
        prompt: String,
        on: StartOn,
    ) -> Result<Arc<AgentLink>> {
        let StartOn {
            model,
            effort,
            checkout,
        } = on;
        let start = Request::Start {
            cwd,
            prompt,
            model,
            effort,
            checkout,
        };
        self.open_agent(agent, start).await
    }

    /// The models `agent` can run on here; `None` from a node too old to say.
    pub(crate) async fn agent_models(&self, agent: &str) -> Result<Option<ModelList>> {
        let (mut tx, mut rx) = self.open_stream(SESSIONS_PROTOCOL).await?;
        tx.send(&SessionsRequest::Models {
            agent: agent.to_owned(),
        })
        .await?;
        let reply = timeout(MODELS_TIMEOUT, rx.recv::<SessionsReply>())
            .await
            .context("the node did not answer")?;
        match reply {
            Ok(Some(SessionsReply::Models { models })) => Ok(Some(models)),
            Ok(Some(SessionsReply::Error { message })) => bail!("{message}"),
            Ok(Some(other)) => bail!("the node answered {other:?}"),
            Ok(None) => Ok(None),
            Err(e) => {
                log::debug!("node did not list models; agent={agent} error={e:#}");
                Ok(None)
            }
        }
    }

    /// Follows `session`, attaching to it when no stream does yet.
    pub async fn agent_session(&self, agent: &str, session: &str) -> Result<Arc<AgentLink>> {
        if let Some(link) = self.agents.live(session) {
            return Ok(link);
        }
        let session = session.to_owned();
        self.open_agent(agent, Request::Attach { session }).await
    }

    /// Takes over and follows a session the user started outside Beep.
    pub async fn take_over(
        &self,
        agent: &str,
        session: &str,
        prompt: Option<String>,
    ) -> Result<Arc<AgentLink>> {
        let session = session.to_owned();
        self.open_agent(agent, Request::Resume { session, prompt })
            .await
    }

    /// Sessions of `agent` started outside Beep, newest first: at most `limit`, or only `only`.
    pub async fn external_sessions(
        &self,
        agent: &str,
        limit: u32,
        only: Option<String>,
    ) -> Result<Vec<ExternalSession>> {
        let request = SessionsRequest::External {
            agent: agent.to_owned(),
            limit,
            session: only,
        };
        match self.sessions_request(request).await {
            Ok(SessionsReply::External { sessions }) => Ok(sessions),
            Ok(SessionsReply::Error { message }) => bail!("{message}"),
            Ok(other) => bail!("the node answered {other:?}"),
            // An older node closes the stream on a request it cannot read.
            Err(e) => Err(e.context("the node may need an update to list them")),
        }
    }

    /// How Claude Code session `session`, started outside Beep, does now, after queuing
    /// `steer` for its next hook when given.
    pub(crate) async fn live_session(
        &self,
        session: &str,
        steer: Option<Steer>,
    ) -> Result<LiveStatus> {
        let session = session.to_owned();
        let request = match steer {
            Some(steer) => SessionsRequest::Steer { session, steer },
            None => SessionsRequest::Live { session },
        };
        match self.sessions_request(request).await {
            Ok(SessionsReply::Live { status }) => Ok(*status),
            Ok(SessionsReply::Error { message }) => bail!("{message}"),
            Ok(other) => bail!("the node answered {other:?}"),
            Err(e) => Err(e.context("the node may need an update for live status")),
        }
    }

    /// The git checkouts coding agents worked in on the node, newest first.
    pub async fn found_folders(&self, limit: u32) -> Result<Vec<SessionFolder>> {
        match self
            .sessions_request(SessionsRequest::Folders { limit })
            .await
        {
            Ok(SessionsReply::Folders { folders }) => Ok(folders),
            Ok(SessionsReply::Error { message }) => bail!("{message}"),
            Ok(other) => bail!("the node answered {other:?}"),
            Err(e) => Err(e.context("the node may need an update to find them")),
        }
    }

    /// The sessions coding agents ran in `folders` or below, newest first.
    pub async fn sessions_in(
        &self,
        folders: Vec<String>,
        limit: u32,
    ) -> Result<Vec<ExternalSession>> {
        let request = SessionsRequest::InFolders { folders, limit };
        match self.sessions_request(request).await {
            Ok(SessionsReply::External { sessions }) => Ok(sessions),
            Ok(SessionsReply::Error { message }) => bail!("{message}"),
            Ok(other) => bail!("the node answered {other:?}"),
            Err(e) => Err(e.context("the node may need an update to list them")),
        }
    }

    pub(super) async fn open_agent(&self, agent: &str, first: Request) -> Result<Arc<AgentLink>> {
        let protocol = agent::protocol(agent);
        let (mut tx, mut rx) = self.open_stream(&protocol).await?;
        tx.send(&first).await?;
        let event: Event = timeout(READ_TIMEOUT, rx.recv())
            .await
            .context("the node did not answer")??
            .context("the node closed the session stream")?;
        let info = match event {
            Event::Attached { session } => *session,
            Event::Refused { reason } => return Err(SessionRefused { reason }.into()),
            other => bail!("the node answered {other:?}"),
        };
        let id = info.id.clone();
        let (state_tx, state) = watch::channel(Watched {
            info,
            ..Watched::default()
        });
        let (orders, orders_rx) = mpsc::channel(ORDERS);
        let waiting = Arc::new(AtomicUsize::new(0));
        let reporter = Reporter {
            app: self.app.clone(),
            links: Arc::downgrade(&self.agents),
            machine: self.connection.remote_id(),
            session: id.clone(),
            agent: agent.to_owned(),
            waiting: waiting.clone(),
            orders: orders.clone(),
            state: state.clone(),
        };
        let task = tokio::spawn(follow(tx, rx, orders_rx, state_tx, reporter));
        let link = Arc::new(AgentLink {
            orders,
            state,
            waiting,
            _task: AbortOnDropHandle::new(task),
        });
        // A stream attached at the same time is replaced and drops.
        self.agents.lock().insert(id, link.clone());
        Ok(link)
    }

    /// Every agent session on the node.
    pub async fn agent_sessions(&self) -> Result<Vec<SessionInfo>> {
        match self.sessions_request(SessionsRequest::List {}).await? {
            SessionsReply::Sessions { sessions } => Ok(sessions),
            SessionsReply::Error { message } => bail!("{message}"),
            other => bail!("the node answered {other:?}"),
        }
    }

    /// The last `last` transcript entries of `session` and how many came before them.
    pub async fn transcript(
        &self,
        session: &str,
        last: u32,
        thoughts: bool,
    ) -> Result<(Vec<Entry>, u32)> {
        let request = SessionsRequest::Transcript {
            session: session.to_owned(),
            last,
            thoughts,
        };
        match self.sessions_request(request).await? {
            SessionsReply::Transcript { entries, earlier } => Ok((entries, earlier)),
            SessionsReply::Error { message } => bail!("{message}"),
            other => bail!("the node answered {other:?}"),
        }
    }

    /// The picture `key` of `session`'s transcript; `None` when the node no longer has it.
    pub(crate) async fn agent_image(&self, session: &str, key: &str) -> Result<Option<NodeImage>> {
        let request = SessionsRequest::Image {
            session: session.to_owned(),
            key: key.to_owned(),
        };
        match self.sessions_request(request).await? {
            SessionsReply::Image { mime, data } => Ok(Some(NodeImage { mime, data })),
            SessionsReply::Error { message } => {
                log::debug!("node has no image; session={session} key={key} reason={message}");
                Ok(None)
            }
            other => bail!("the node answered {other:?}"),
        }
    }

    /// Part of the media file `path` on the node, from `offset`.
    pub(crate) async fn agent_file(&self, path: &str, offset: u64) -> Result<NodeFile> {
        let request = SessionsRequest::File {
            path: path.to_owned(),
            offset,
        };
        match self.sessions_request(request).await? {
            SessionsReply::File { size, mime, data } => Ok(NodeFile::Chunk { size, mime, data }),
            SessionsReply::NoFile {} => Ok(NodeFile::Missing),
            SessionsReply::Error { message } => Ok(NodeFile::Refused(message)),
            other => bail!("the node answered {other:?}"),
        }
    }

    /// The folder `path` inside the session folder `root` on the node.
    pub(crate) async fn work_folder(
        &self,
        root: &str,
        path: &str,
        ignored: bool,
    ) -> Result<NodeWork<WorkFolder>> {
        let request = SessionsRequest::Folder {
            root: root.to_owned(),
            path: path.to_owned(),
            ignored,
            depth: 0,
        };
        match self.sessions_request(request).await? {
            SessionsReply::Folder { folder } => Ok(NodeWork::Done(folder)),
            SessionsReply::Error { message } => Ok(NodeWork::Refused(message)),
            _ => bail!("the node answered something else"),
        }
    }

    /// The folder `ask.path` inside `ask.root` on the node and those under
    /// it, `ask.depth` levels in all; `ask.path` alone from a node from
    /// before trees.
    pub(crate) async fn work_tree(&self, ask: &ListWorkTree) -> Result<NodeWork<WorkTree>> {
        let request = SessionsRequest::Folder {
            root: ask.root.to_owned(),
            path: ask.path.to_owned(),
            ignored: ask.ignored,
            depth: ask.depth.max(1),
        };
        match self.sessions_request(request).await? {
            SessionsReply::Tree { tree } => Ok(NodeWork::Done(tree)),
            SessionsReply::Folder { folder } => Ok(NodeWork::Done(WorkTree {
                folders: vec![folder],
                truncated: false,
            })),
            SessionsReply::Error { message } => Ok(NodeWork::Refused(message)),
            _ => bail!("the node answered something else"),
        }
    }

    /// Part of a file inside a session folder on the node.
    pub(crate) async fn read_work_file(&self, read: WorkRead<'_>) -> Result<NodeFile> {
        let request = SessionsRequest::ReadFile {
            root: read.root.to_owned(),
            path: read.path.to_owned(),
            offset: read.offset,
            len: read.len,
        };
        match self.sessions_request(request).await? {
            SessionsReply::File { size, mime, data } => Ok(NodeFile::Chunk { size, mime, data }),
            SessionsReply::NoFile {} => Ok(NodeFile::Missing),
            SessionsReply::Error { message } => Ok(NodeFile::Refused(message)),
            _ => bail!("the node answered something else"),
        }
    }

    /// Writes one part of a file into a session folder on the node; gives the path it went to.
    pub(crate) async fn write_work_file(&self, write: WorkWrite) -> Result<NodeWork<String>> {
        let request = SessionsRequest::WriteFile {
            root: write.root,
            path: write.path,
            offset: write.offset,
            size: write.size,
            data: write.data,
        };
        match self.sessions_request(request).await? {
            SessionsReply::Written { path } => Ok(NodeWork::Done(path)),
            SessionsReply::Error { message } => Ok(NodeWork::Refused(message)),
            _ => bail!("the node answered something else"),
        }
    }

    /// The custom agents the harnesses define for `folder` on the node and in its home.
    pub(crate) async fn custom_agents(&self, folder: &str) -> Result<NodeWork<Vec<CustomAgent>>> {
        let request = SessionsRequest::CustomAgents {
            folder: folder.to_owned(),
        };
        match self.sessions_request(request).await? {
            SessionsReply::CustomAgents { agents } => Ok(NodeWork::Done(agents)),
            SessionsReply::Error { message } => Ok(NodeWork::Refused(message)),
            _ => bail!("the node answered something else"),
        }
    }

    /// What changed in the checkout `ask.root` on the node.
    pub(crate) async fn work_diff(&self, ask: DiffAsk) -> Result<NodeWork<WorkDiff>> {
        match self
            .sessions_request_within(SessionsRequest::Diff { ask }, DIFF_TIMEOUT)
            .await?
        {
            SessionsReply::Diff { diff } => Ok(NodeWork::Done(diff)),
            SessionsReply::Error { message } => Ok(NodeWork::Refused(message)),
            _ => bail!("the node answered something else"),
        }
    }

    pub(super) async fn sessions_request(&self, request: SessionsRequest) -> Result<SessionsReply> {
        self.sessions_request_within(request, READ_TIMEOUT).await
    }

    /// Sends `request` and gives the node's answer within `limit`. When the
    /// link drops or was replaced, a read is sent again on the node's new
    /// connection once it reconnects, and any request the node never got is
    /// too; see `Retry`.
    pub(super) async fn sessions_request_within(
        &self,
        request: SessionsRequest,
        limit: Duration,
    ) -> Result<SessionsReply> {
        let read = request.is_read();
        let until = Instant::now() + RECONNECT_WAIT;
        let mut next: Option<Online> = None;
        loop {
            let on = next.as_ref().unwrap_or(self);
            let failed = match on.sessions_attempt(&request, limit).await {
                Ok(reply) => return Ok(reply),
                Err(failed) => failed,
            };
            let wait_until = match failed.kind.retry(read) {
                Retry::No => return Err(failed.error),
                Retry::IfReplaced => Instant::now(),
                Retry::AfterReconnect => until,
            };
            let used = on.connection.stable_id();
            let nodes = self.app.upgrade();
            let back = match &nodes {
                Some(app) => {
                    app.nodes
                        .reconnected(&on.connection.remote_id(), Some(used), wait_until)
                        .await
                }
                None => None,
            };
            let Some(back) = back else {
                return Err(failed.error);
            };
            log::info!(
                "nodes: sending a request again on a new connection; node={} error={:#}",
                self.name,
                failed.error
            );
            next = Some(back);
        }
    }

    /// One try of `sessions_request_within` on this connection.
    pub(super) async fn sessions_attempt(
        &self,
        request: &SessionsRequest,
        limit: Duration,
    ) -> Result<SessionsReply, Failed> {
        let (mut tx, mut rx) = self
            .try_open(SESSIONS_PROTOCOL)
            .await
            .map_err(|e| match e {
                Opening::Link(error) => Failed::new(Failure::Unsent, error),
                Opening::Refused(error) => Failed::new(Failure::Answered, error),
            })?;
        tx.send(request)
            .await
            .map_err(|e| Failed::new(Failure::Unsent, e))?;
        // A closed connection says the link dropped, rather than a node
        // that could not read the request.
        let dropped = || {
            if self.connection.close_reason().is_some() {
                Failure::Dropped
            } else {
                Failure::Answered
            }
        };
        match timeout(limit, rx.recv()).await {
            Err(elapsed) => Err(Failed::new(
                Failure::Silent,
                anyhow::Error::new(elapsed).context("the node did not answer"),
            )),
            Ok(Err(e)) => Err(Failed::new(dropped(), e)),
            Ok(Ok(None)) => Err(Failed::new(
                dropped(),
                anyhow::anyhow!("the node closed the stream"),
            )),
            Ok(Ok(Some(reply))) => Ok(reply),
        }
    }

    /// Opens a stream for `protocol`; a link that fails first gives an
    /// `Unsent` error, so the call may go again.
    pub(in crate::nodes) async fn open_stream(&self, protocol: &str) -> Result<(LinkTx, LinkRx)> {
        self.try_open(protocol).await.map_err(|e| match e {
            Opening::Link(e) => Unsent(e).into(),
            Opening::Refused(e) => e,
        })
    }

    pub(super) async fn try_open(&self, protocol: &str) -> Result<(LinkTx, LinkRx), Opening> {
        if !self.offers(protocol) {
            return Err(Opening::Refused(anyhow::anyhow!(
                "{} does not offer {protocol}",
                self.name
            )));
        }
        PeerClient::new(self.connection.clone())
            .open(protocol)
            .await
            .map_err(|e| match e {
                MeshError::Declined { .. } | MeshError::Unexpected(_) => Opening::Refused(e.into()),
                e => Opening::Link(e.into()),
            })
    }
}
