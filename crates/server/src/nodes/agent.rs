//! Coding-agent sessions on a node: one followed stream per session, keeping its latest state.

use super::{Ask, Follower, Online, RECONNECT_WAIT};
use crate::{
    api::transport::READ_TIMEOUT,
    protocol::node::{
        LinkRx, LinkTx, Open, Opened,
        agent::{
            self, Entry, Event, ExternalSession, LiveStatus, ModelList, Question, Request,
            SESSIONS_PROTOCOL, SessionFolder, SessionInfo, SessionsReply, SessionsRequest, Status,
            Steer, StopReason,
        },
        link,
    },
    protocol::{CustomAgent, ListWorkTree, WorkDiff, WorkFolder, WorkTree, node::agent::DiffAsk},
};
use anyhow::{Context, Result, bail};
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    sync::{mpsc, oneshot, watch},
    time::{Instant, timeout},
};
use tokio_util::task::AbortOnDropHandle;
use wake::Reporter;

pub(crate) use wake::News;

mod wake;

/// The node refused the session stream's first request (for an attach: it lost the session).
#[derive(Debug, thiserror::Error)]
#[error("{reason}")]
pub(crate) struct SessionRefused {
    pub(crate) reason: String,
}

/// A picture of a session's transcript, as the node gave it.
pub(crate) struct NodeImage {
    pub(crate) mime: String,
    pub(crate) data: Vec<u8>,
}

/// What a node answered when asked for part of a media file.
pub(crate) enum NodeFile {
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
pub(crate) enum NodeWork<T> {
    Done(T),
    /// The node would not, saying why.
    Refused(String),
}

/// A read of part of a file in a session's folder.
pub(crate) struct WorkRead<'a> {
    pub(crate) root: &'a str,
    pub(crate) path: &'a str,
    pub(crate) offset: u64,
    pub(crate) len: u32,
}

/// One part of a file written into a session's folder.
pub(crate) struct WorkWrite {
    pub(crate) root: String,
    pub(crate) path: String,
    pub(crate) offset: u64,
    pub(crate) size: u64,
    pub(crate) data: Vec<u8>,
}

/// What a session starts on besides its folder and prompt; `None` keeps the agent's default.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StartOn {
    /// An id from the agent's model list.
    pub model: Option<String>,
    /// One of `agent::efforts` for the agent.
    pub effort: Option<agent::Effort>,
    /// The branch or worktree to make first, for a node that makes them.
    pub checkout: Option<agent::CheckoutPlan>,
}

/// How long a node has to list models: it may start the agent and npx may download it first.
const MODELS_TIMEOUT: Duration = Duration::from_secs(60);
/// How long a node has to read a diff: git may walk a large checkout.
const DIFF_TIMEOUT: Duration = Duration::from_secs(60);

/// Unreadable messages in a row before a link gives up.
const MAX_BAD_MESSAGES: u32 = 8;
const ORDERS: usize = 8;

/// The most sessions on one node that report to a chat.
const MAX_OWNERS: usize = 64;

/// The sessions a node's links follow, by id, and the chats they report to.
#[derive(Default)]
pub struct AgentLinks {
    links: Mutex<HashMap<String, Arc<AgentLink>>>,
    owners: Mutex<HashMap<String, Follower>>,
}

impl AgentLinks {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, Arc<AgentLink>>> {
        self.links.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn owners(&self) -> MutexGuard<'_, HashMap<String, Follower>> {
        self.owners.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The chat `session` reports to.
    pub(super) fn owner(&self, session: &str) -> Option<Follower> {
        self.owners().get(session).cloned()
    }

    fn set_owner(&self, session: &str, owner: Follower) {
        let mut owners = self.owners();
        if !owners.contains_key(session) && owners.len() >= MAX_OWNERS {
            // Sessions that ended or went away keep their place until here.
            let links = self.lock();
            owners.retain(|id, _| links.get(id).is_some_and(|link| !link.is_lost()));
        }
        if owners.contains_key(session) || owners.len() < MAX_OWNERS {
            owners.insert(session.to_owned(), owner);
        } else {
            log::warn!("nodes: too many sessions report to chats; {session} will not");
        }
    }

    fn live(&self, session: &str) -> Option<Arc<AgentLink>> {
        let mut links = self.lock();
        match links.get(session) {
            Some(link) if !link.is_lost() => Some(link.clone()),
            Some(_) => {
                links.remove(session);
                None
            }
            None => None,
        }
    }
}

/// A session as the server last heard of it.
#[derive(Debug, Clone, Default)]
pub struct Watched {
    pub info: SessionInfo,
    pub last_stop: Option<StopReason>,
    /// Why the session ended, once it has.
    pub ended: Option<String>,
    /// The stream dropped; the session may still run on the node.
    pub lost: bool,
}

/// Where a session was before an order, to wait for what comes after.
#[derive(Debug, Clone, Copy)]
pub struct Mark {
    turns: u32,
    question: Option<u32>,
}

impl Watched {
    pub fn mark(&self) -> Mark {
        Mark {
            turns: self.info.turns,
            question: self.info.question.as_ref().map(|question| question.id),
        }
    }

    /// A turn ended, a new question came, or the session is gone since `mark`.
    fn moved_on(&self, mark: Mark) -> bool {
        let question = self.info.question.as_ref().map(|question| question.id);
        self.ended.is_some()
            || self.lost
            || self.info.turns > mark.turns
            || (question.is_some() && question != mark.question)
    }

    /// Nothing will happen until someone acts.
    pub fn settled(&self) -> bool {
        self.ended.is_some()
            || self.lost
            || matches!(
                self.info.state.status,
                Status::Idle | Status::Asking | Status::Ended
            )
    }
}

/// One followed session.
pub struct AgentLink {
    orders: mpsc::Sender<LinkOrder>,
    state: watch::Receiver<Watched>,
    /// Calls waiting for the session; while one does, no report goes to the chat.
    waiting: Arc<AtomicUsize>,
    _task: AbortOnDropHandle<()>,
}

struct LinkOrder {
    request: Request,
    done: oneshot::Sender<Result<(), String>>,
}

impl AgentLink {
    pub fn now(&self) -> Watched {
        self.state.borrow().clone()
    }

    /// The card for the session's open `question` in `follower`'s chat.
    pub(crate) fn ask(&self, follower: Follower, question: Question, task: String) -> Ask {
        let session = self.state.borrow().info.id.clone();
        wake::ask_card(
            &self.state,
            &self.orders,
            session,
            follower,
            question,
            Some(task),
        )
    }

    fn is_lost(&self) -> bool {
        let state = self.state.borrow();
        state.lost || state.ended.is_some()
    }

    /// Sends `request` and waits for the node to take it; cancelling leaves the stream whole.
    pub async fn send(&self, request: Request) -> Result<()> {
        order(&self.orders, request).await
    }

    /// Waits up to `limit` for the session to move on from `mark`, then gives its state.
    pub async fn wait(&self, mark: Mark, limit: Duration) -> Watched {
        let _waiting = Waiting::new(&self.waiting);
        let mut state = self.state.clone();
        // Timing out or losing the task both leave the latest state.
        let _ = timeout(limit, state.wait_for(|now| now.moved_on(mark))).await;
        self.now()
    }
}

/// Counts a call waiting for a session while it lives.
struct Waiting(Arc<AtomicUsize>);

impl Waiting {
    fn new(count: &Arc<AtomicUsize>) -> Self {
        count.fetch_add(1, Ordering::SeqCst);
        Self(count.clone())
    }
}

impl Drop for Waiting {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Sends `request` down a session's stream and waits for the node to take it.
async fn order(orders: &mpsc::Sender<LinkOrder>, request: Request) -> Result<()> {
    let (done, answer) = oneshot::channel();
    orders
        .send(LinkOrder { request, done })
        .await
        .map_err(|_| anyhow::anyhow!("lost the session's stream"))?;
    let taken = timeout(READ_TIMEOUT, answer)
        .await
        .context("the node did not answer")?
        .map_err(|_| anyhow::anyhow!("lost the session's stream"))?;
    taken.map_err(anyhow::Error::msg)
}

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

    async fn open_agent(&self, agent: &str, first: Request) -> Result<Arc<AgentLink>> {
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

    async fn sessions_request(&self, request: SessionsRequest) -> Result<SessionsReply> {
        self.sessions_request_within(request, READ_TIMEOUT).await
    }

    /// Sends `request` and gives the node's answer within `limit`. When the
    /// link drops or was replaced, a read is sent again on the node's new
    /// connection once it reconnects, and any request the node never got is
    /// too; see `Retry`.
    async fn sessions_request_within(
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
    async fn sessions_attempt(
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

    pub(super) async fn open_stream(&self, protocol: &str) -> Result<(LinkTx, LinkRx)> {
        self.try_open(protocol).await.map_err(|e| match e {
            Opening::Link(e) | Opening::Refused(e) => e,
        })
    }

    async fn try_open(&self, protocol: &str) -> Result<(LinkTx, LinkRx), Opening> {
        if !self.offers(protocol) {
            return Err(Opening::Refused(anyhow::anyhow!(
                "{} does not offer {protocol}",
                self.name
            )));
        }
        let link_error = |e: anyhow::Error| Opening::Link(e);
        let (send, recv) = self
            .connection
            .open_bi()
            .await
            .map_err(|e| link_error(e.into()))?;
        let (mut tx, mut rx) = link(send, recv);
        tx.send(&Open {
            protocol: protocol.to_owned(),
        })
        .await
        .map_err(link_error)?;
        let opened: Opened = timeout(READ_TIMEOUT, rx.recv())
            .await
            .context("the node did not open the stream")
            .map_err(link_error)?
            .map_err(link_error)?
            .context("the node closed the stream")
            .map_err(link_error)?;
        match opened {
            Opened::Ready {} => Ok((tx, rx)),
            Opened::Refused { reason } => Err(Opening::Refused(anyhow::anyhow!(
                "the node refused {protocol}: {reason}"
            ))),
            other => Err(Opening::Refused(anyhow::anyhow!(
                "the node answered {other:?}"
            ))),
        }
    }
}

/// Why a stream did not open.
enum Opening {
    /// The link failed before the node took the stream; nothing was sent
    /// on it.
    Link(anyhow::Error),
    /// The node does not serve the protocol, or said no.
    Refused(anyhow::Error),
}

/// A try of a sessions request that failed, and how.
struct Failed {
    kind: Failure,
    error: anyhow::Error,
}

impl Failed {
    fn new(kind: Failure, error: anyhow::Error) -> Self {
        Self { kind, error }
    }
}

/// How a sessions request failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Failure {
    /// The request never reached the node: the stream did not open or the
    /// request could not be written.
    Unsent,
    /// The connection closed after the request went out; the node may
    /// have acted on it.
    Dropped,
    /// No answer in time on a connection still open.
    Silent,
    /// The node answered, refused or closed the stream itself.
    Answered,
}

/// Whether a failed request goes again on the node's next connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Retry {
    No,
    /// Only on a newer connection the node has made already.
    IfReplaced,
    /// On the node's next connection, waiting for it up to
    /// `RECONNECT_WAIT`.
    AfterReconnect,
}

impl Failure {
    /// A request the node never got may always go again; a read lost with
    /// its link may too. A read on a live link that went unanswered goes
    /// again only when the node has reconnected meanwhile: on the same
    /// link it would likely hang again.
    fn retry(self, read: bool) -> Retry {
        match self {
            Self::Unsent => Retry::AfterReconnect,
            Self::Dropped if read => Retry::AfterReconnect,
            Self::Silent if read => Retry::IfReplaced,
            Self::Dropped | Self::Silent | Self::Answered => Retry::No,
        }
    }
}

/// Reads a session stream into `state` and writes the orders, until either ends.
async fn follow(
    mut tx: LinkTx,
    mut rx: LinkRx,
    mut orders: mpsc::Receiver<LinkOrder>,
    state: watch::Sender<Watched>,
    reporter: Reporter,
) {
    let mut acks: VecDeque<oneshot::Sender<Result<(), String>>> = VecDeque::new();
    let mut bad = 0;
    loop {
        tokio::select! {
            order = orders.recv() => {
                let Some(LinkOrder { request, done }) = order else {
                    return;
                };
                if let Err(e) = tx.send(&request).await {
                    log::debug!("agent stream lost: {e:#}");
                    break;
                }
                acks.push_back(done);
            }
            event = rx.recv::<Event>() => match event {
                Ok(Some(event)) => {
                    bad = 0;
                    if let Event::Ack { error } = event {
                        if let Some(done) = acks.pop_front() {
                            // The caller may have stopped waiting.
                            let _ = done.send(error.map_or(Ok(()), Err));
                        }
                        continue;
                    }
                    let ended = matches!(event, Event::Ended { .. });
                    let news = News::of(&event);
                    state.send_modify(|watched| apply(watched, event));
                    if let Some(news) = news {
                        reporter.tell(news);
                    }
                    if ended {
                        return;
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    log::debug!("agent stream message skipped: {e:#}");
                    bad += 1;
                    if bad >= MAX_BAD_MESSAGES {
                        break;
                    }
                }
            },
        }
    }
    state.send_modify(|watched| watched.lost = true);
}

fn apply(watched: &mut Watched, event: Event) {
    let info = &mut watched.info;
    match event {
        Event::State { state } => {
            if state.status != Status::Asking {
                info.question = None;
            }
            info.state = state;
        }
        Event::Question { question } => {
            info.question = Some(question);
            info.state.status = Status::Asking;
        }
        Event::TurnDone {
            turn,
            stop_reason,
            last_message,
        } => {
            info.turns = turn;
            info.last_message = last_message;
            watched.last_stop = Some(stop_reason);
        }
        Event::Ended { reason } => {
            info.state.status = Status::Ended;
            info.question = None;
            watched.ended = Some(reason);
        }
        other => log::debug!("agent stream sent something unhandled: {other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::node::agent::{AnswerOption, OptionKind, State};

    fn question(id: u32) -> Question {
        Question {
            id,
            text: "Edit notes.txt".into(),
            detail: None,
            options: vec![AnswerOption {
                id: "allow".into(),
                name: "Allow".into(),
                kind: OptionKind::AllowOnce,
            }],
        }
    }

    #[test]
    fn a_wait_ends_on_a_turn_a_new_question_or_the_end() {
        let mut watched = Watched::default();
        apply(
            &mut watched,
            Event::State {
                state: State {
                    status: Status::Working,
                    ..State::default()
                },
            },
        );
        let mark = watched.mark();
        assert!(!watched.moved_on(mark));
        apply(
            &mut watched,
            Event::Question {
                question: question(1),
            },
        );
        assert!(watched.moved_on(mark));
        assert_eq!(watched.info.state.status, Status::Asking);

        let mark = watched.mark();
        assert!(
            !watched.moved_on(mark),
            "the same question again is not news"
        );
        apply(
            &mut watched,
            Event::State {
                state: State {
                    status: Status::Working,
                    ..State::default()
                },
            },
        );
        assert!(watched.info.question.is_none(), "an answered question goes");
        apply(
            &mut watched,
            Event::TurnDone {
                turn: 1,
                stop_reason: StopReason::EndTurn,
                last_message: "Done".into(),
            },
        );
        assert!(watched.moved_on(mark));
        assert_eq!(watched.info.last_message, "Done");

        let mark = watched.mark();
        apply(
            &mut watched,
            Event::Ended {
                reason: "closed".into(),
            },
        );
        assert!(watched.moved_on(mark));
        assert!(watched.settled());
    }

    #[tokio::test]
    async fn a_waiting_call_keeps_reports_out_of_the_chat_until_it_returns() {
        let (state_tx, state) = watch::channel(Watched::default());
        let (orders, _orders) = mpsc::channel(1);
        let link = AgentLink {
            orders,
            state,
            waiting: Arc::default(),
            _task: AbortOnDropHandle::new(tokio::spawn(async {})),
        };
        let wait = link.wait(link.now().mark(), Duration::from_secs(600));
        tokio::pin!(wait);
        assert!(futures::poll!(&mut wait).is_pending());
        assert_eq!(link.waiting.load(Ordering::SeqCst), 1);
        state_tx.send_modify(|watched| {
            apply(
                watched,
                Event::TurnDone {
                    turn: 1,
                    stop_reason: StopReason::EndTurn,
                    last_message: String::new(),
                },
            )
        });
        assert_eq!(wait.await.info.turns, 1);
        assert_eq!(link.waiting.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn reads_go_again_after_a_reconnect_and_writes_only_when_unsent() {
        use {Failure::*, Retry::*};
        assert_eq!(Unsent.retry(true), AfterReconnect);
        assert_eq!(Unsent.retry(false), AfterReconnect);
        assert_eq!(Dropped.retry(true), AfterReconnect);
        assert_eq!(Dropped.retry(false), No);
        assert_eq!(Silent.retry(true), IfReplaced);
        assert_eq!(Silent.retry(false), No);
        assert_eq!(Answered.retry(true), No);
        assert_eq!(Answered.retry(false), No);
    }
}
