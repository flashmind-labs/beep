//! The coding-agent requests a server makes of one online node.

use super::{
    super::{Follower, Online, RECONNECT_WAIT, retry::Unsent},
    Failed, Failure, ORDERS, Opening, Retry, follow,
    wake::Reporter,
};
use crate::{
    api::transport::READ_TIMEOUT,
    nodes::{
        AgentLink, NodeFile, NodeImage, NodeWork, SessionRefused, StartOn, Watched, WorkRead,
        WorkWrite,
    },
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
use beep_mesh::{
    MeshError, PeerClient,
    machine::{Sessions, sessions},
};
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

impl Sessions for Online {
    async fn sessions(
        &self,
        request: SessionsRequest,
        limit: Duration,
    ) -> Result<SessionsReply, MeshError> {
        self.sessions_request_within(request, limit)
            .await
            .map_err(MeshError::Link)
    }
}

/// The requests to a node's sessions, which the desktop makes the same way.
impl Online {
    pub(crate) async fn agent_models(&self, agent: &str) -> Result<Option<ModelList>> {
        Ok(sessions::models(self, agent).await?)
    }

    pub async fn agent_sessions(&self) -> Result<Vec<SessionInfo>> {
        Ok(sessions::list(self).await?)
    }

    pub async fn transcript(
        &self,
        session: &str,
        last: u32,
        thoughts: bool,
    ) -> Result<(Vec<Entry>, u32)> {
        Ok(sessions::transcript(self, session, last, thoughts).await?)
    }

    pub async fn external_sessions(
        &self,
        agent: &str,
        limit: u32,
        only: Option<String>,
    ) -> Result<Vec<ExternalSession>> {
        Ok(sessions::external(self, agent, limit, only).await?)
    }

    pub(crate) async fn live_session(
        &self,
        session: &str,
        steer: Option<Steer>,
    ) -> Result<LiveStatus> {
        Ok(sessions::live(self, session, steer).await?)
    }

    pub async fn found_folders(&self, limit: u32) -> Result<Vec<SessionFolder>> {
        Ok(sessions::found_folders(self, limit).await?)
    }

    pub async fn sessions_in(
        &self,
        folders: Vec<String>,
        limit: u32,
    ) -> Result<Vec<ExternalSession>> {
        Ok(sessions::in_folders(self, folders, limit).await?)
    }

    pub(crate) async fn agent_image(&self, session: &str, key: &str) -> Result<Option<NodeImage>> {
        Ok(sessions::image(self, session, key).await?)
    }

    pub(crate) async fn agent_file(&self, path: &str, offset: u64) -> Result<NodeFile> {
        Ok(sessions::media(self, path, offset).await?)
    }

    pub(crate) async fn work_folder(
        &self,
        root: &str,
        path: &str,
        ignored: bool,
    ) -> Result<NodeWork<WorkFolder>> {
        Ok(sessions::folder(self, root, path, ignored).await?)
    }

    pub(crate) async fn work_tree(&self, ask: &ListWorkTree) -> Result<NodeWork<WorkTree>> {
        Ok(sessions::tree(self, ask).await?)
    }

    pub(crate) async fn read_work_file(&self, read: WorkRead<'_>) -> Result<NodeFile> {
        Ok(sessions::read_file(self, read).await?)
    }

    pub(crate) async fn write_work_file(&self, write: WorkWrite) -> Result<NodeWork<String>> {
        Ok(sessions::write_file(self, write).await?)
    }

    pub(crate) async fn custom_agents(&self, folder: &str) -> Result<NodeWork<Vec<CustomAgent>>> {
        Ok(sessions::custom_agents(self, folder).await?)
    }

    pub(crate) async fn work_diff(&self, ask: DiffAsk) -> Result<NodeWork<WorkDiff>> {
        Ok(sessions::diff(self, ask).await?)
    }
}
