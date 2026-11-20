//! The bridge's task: ACP from the client on one side, Hermes's dashboard
//! socket on the other. One session per bridge, as the node runs them. The
//! session outlives the socket: a dropped socket is reconnected and the
//! session resumed, with what was missed meanwhile.

use super::{
    ask::{self, Asked},
    events,
    link::{Gateway, Incoming, LinkError},
};
use crate::agents::acp;
use agent_client_protocol::{
    Agent, Builder, Client, ConnectionTo, Dispatch, Handled, JsonRpcMessage, JsonRpcResponse, RequestCancellationHandle,
    Responder, UntypedMessage, on_receive_dispatch,
    schema::ProtocolVersion,
    v1::schema::{
        AgentCapabilities, CLIENT_METHOD_NAMES, CancelNotification, ContentBlock, EmbeddedResourceResource,
        Implementation, InitializeResponse, LoadSessionRequest, LoadSessionResponse, NewSessionResponse,
        PromptRequest, PromptResponse, ResumeSessionRequest, ResumeSessionResponse, SessionCapabilities, SessionId,
        SessionResumeCapabilities, StopReason,
    },
};
use anyhow::{Context, Result};
use beep_proto::{SecretText, node::agent::HERMES_GATEWAY};
use futures::{FutureExt, StreamExt, future::BoxFuture, stream::FuturesUnordered};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::mpsc,
};

/// How many messages from the client wait to be handled.
const EVENTS: usize = 64;
/// How long a try at reconnecting may take, and the wait before the next.
const TRY: Duration = Duration::from_secs(10);
#[cfg(not(test))]
const RETRY: Duration = Duration::from_secs(5);
#[cfg(test)]
const RETRY: Duration = Duration::from_millis(10);

/// Serves ACP read from `input` and written to `output`, driving Hermes's
/// dashboard on `host` and `port` with `token`, until the client closes it.
pub(in crate::agents) async fn serve(
    input: impl AsyncRead + Unpin + Send + 'static,
    output: impl AsyncWrite + Unpin + Send + 'static,
    host: String,
    port: u16,
    token: SecretText,
) -> Result<()> {
    let (events_tx, mut events) = mpsc::channel(EVENTS);
    // Its reports go unread: a dropped message is logged where it is read.
    let (oversized, _) = mpsc::channel(1);

    Builder::new(Agent)
        .name(HERMES_GATEWAY)
        .on_receive_dispatch(
            async move |dispatch: Dispatch, _cx| {
                let event = match dispatch {
                    Dispatch::Request(message, responder) => Event::Request(message, Box::new(responder)),
                    Dispatch::Notification(message) => Event::Notification(message),
                    dispatch => {
                        return Ok(Handled::No {
                            message: dispatch,
                            retry: false,
                        });
                    }
                };
                let _ = events_tx.send(event).await;
                Ok(Handled::Yes)
            },
            on_receive_dispatch!(),
        )
        .connect_with(acp::transport(output, input, oversized), async |cx: ConnectionTo<Client>| {
            let mut bridge = Bridge {
                cx,
                hermes: Hermes { host, port, token },
                session: None,
                answers: FuturesUnordered::new(),
            };

            bridge.run(&mut events).await.map_err(|e| {
                log::error!("{e:#}");
                agent_client_protocol::Error::internal_error().data(format!("{e:#}"))
            })
        })
        .await
        .context("the ACP connection failed")
}

/// What the client sent.
enum Event {
    Request(UntypedMessage, Box<Responder>),
    Notification(UntypedMessage),
}

/// Where Hermes is reached.
#[derive(Clone)]
struct Hermes {
    host: String,
    port: u16,
    token: SecretText,
}

/// The socket to Hermes, or the reconnection that brings it back.
enum Link {
    Up(Gateway),
    Down(BoxFuture<'static, Resumed>),
}

/// A session resumed on a new socket: its live id and what Hermes sent
/// since the sequence asked for.
struct Resumed {
    gateway: Gateway,
    live: String,
    since: Value,
}

/// What happened on the link.
enum Happened {
    Incoming(Incoming),
    Dropped,
    Resumed(Resumed),
}

/// The client's answer to Hermes's request `id`.
struct Answer {
    id: String,
    result: Value,
}

struct Session {
    /// Hermes's stored session id, which outlives the socket.
    id: SessionId,
    /// The id of the session on the current socket.
    live: String,
    link: Link,
    /// The sequence of the last event seen.
    seq: u64,
    /// Hermes's replay epoch, which changes when Hermes restarts.
    epoch: Value,
    prompt: Option<Responder<PromptResponse>>,
    cancelled: bool,
    /// A turn stopped while the socket was down: Hermes is told to stop it
    /// once the socket is back.
    interrupt: bool,
    /// Hermes's requests the client is answering, each with the way to
    /// withdraw it.
    asking: HashMap<String, RequestCancellationHandle>,
    /// Answers made while the socket was down, sent once it is back.
    held: Vec<Answer>,
}

struct Bridge {
    cx: ConnectionTo<Client>,
    hermes: Hermes,
    session: Option<Session>,
    answers: FuturesUnordered<BoxFuture<'static, Answer>>,
}

impl Bridge {
    async fn run(&mut self, events: &mut mpsc::Receiver<Event>) -> Result<()> {
        loop {
            tokio::select! {
                event = events.recv() => match event {
                    Some(Event::Request(message, responder)) => self.request(message, *responder).await?,
                    Some(Event::Notification(message)) => self.notification(&message).await?,
                    None => return Ok(()),
                },
                () = self.cx.incoming_closed() => return Ok(()),
                happened = next(&mut self.session) => match happened {
                    Happened::Incoming(incoming) => self.incoming(incoming)?,
                    Happened::Dropped => self.dropped(),
                    Happened::Resumed(resumed) => self.resumed(resumed).await?,
                },
                Some(answer) = self.answers.next() => self.answered(answer),
            }
        }
    }

    async fn request(&mut self, message: UntypedMessage, responder: Responder) -> Result<()> {
        let method = message.method().to_owned();
        let params = message.params();

        match method.as_str() {
            "initialize" => responder.cast().respond(initialized())?,
            "session/new" | "session/load" | "session/resume" if self.session.is_some() => {
                responder.respond_with_error(agent_client_protocol::Error::invalid_request().data("this process runs one session"))?;
            }
            "session/new" => {
                let responder = responder.cast::<NewSessionResponse>();
                match self.open(None).await {
                    Ok((id, _)) => responder.respond(NewSessionResponse::new(id))?,
                    Err(e) => return refused(responder, e),
                }
            }
            "session/load" => {
                let request = LoadSessionRequest::parse_message(&method, params)?;
                let responder = responder.cast::<LoadSessionResponse>();
                match self.open(Some((request.session_id, true))).await {
                    Ok((_, since)) => {
                        responder.respond(LoadSessionResponse::new())?;
                        self.ask_open(&since, &HashSet::new())?;
                    }
                    Err(e) => return refused(responder, e),
                }
            }
            "session/resume" => {
                let request = ResumeSessionRequest::parse_message(&method, params)?;
                let responder = responder.cast::<ResumeSessionResponse>();
                match self.open(Some((request.session_id, false))).await {
                    Ok((_, since)) => {
                        responder.respond(ResumeSessionResponse::new())?;
                        self.ask_open(&since, &HashSet::new())?;
                    }
                    Err(e) => return refused(responder, e),
                }
            }
            "session/prompt" => {
                let request = PromptRequest::parse_message(&method, params)?;
                self.prompt(&request.session_id, &request.prompt, responder.cast()).await?;
            }
            _ => responder.respond_with_error(agent_client_protocol::Error::method_not_found())?,
        }

        Ok(())
    }

    async fn notification(&mut self, message: &UntypedMessage) -> Result<()> {
        if message.method() != "session/cancel" {
            return Ok(());
        }

        let Ok(cancel) = CancelNotification::parse_message(message.method(), message.params()) else {
            return Ok(());
        };
        let Some(session) = self.session.as_mut().filter(|session| session.id == cancel.session_id) else {
            return Ok(());
        };
        if session.prompt.is_none() || session.cancelled {
            return Ok(());
        }

        // While the socket is down, the turn ends at once and Hermes is told
        // to stop it once the socket is back. A turn Hermes cannot stop is
        // not waited on.
        session.cancelled = true;
        match &session.link {
            Link::Up(gateway) if interrupt(gateway, &session.live).await.is_ok() => {}
            Link::Up(_) => ended(session, &Value::Null)?,
            Link::Down(_) => {
                session.interrupt = true;
                ended(session, &Value::Null)?;
            }
        }

        Ok(())
    }

    /// Opens the session: a new one, or the stored one `resume` names,
    /// replaying what Hermes kept of it when asked. Yields its id and what
    /// Hermes sent since, whose open requests are asked once the client
    /// knows the session.
    async fn open(&mut self, resume: Option<(SessionId, bool)>) -> Result<(SessionId, Value)> {
        let gateway = Gateway::connect(&self.hermes.host, self.hermes.port, &self.hermes.token).await?;
        let (id, live, since) = match resume {
            None => {
                // Hermes works in its own folder: no `cwd`.
                let created = gateway.call("session.create", json!({})).await?;
                let id = created["stored_session_id"].as_str().context("Hermes made no stored session")?;
                let live = created["session_id"].as_str().context("Hermes made no session")?;
                // Read for its epoch, to tell a restart of Hermes later.
                let since = gateway.call("session.events.since", json!({"session_id": live, "last_seen": 0})).await?;
                (SessionId::new(id), live.to_owned(), (since, false))
            }
            Some((id, replay)) => {
                let (live, since) = resume_on(&gateway, &id.0, 0).await?;
                (id, live, (since, replay))
            }
        };
        let (since, replay) = since;

        self.session = Some(Session {
            id: id.clone(),
            live,
            link: Link::Up(gateway),
            seq: 0,
            epoch: since["epoch"].clone(),
            prompt: None,
            cancelled: false,
            interrupt: false,
            asking: HashMap::new(),
            held: Vec::new(),
        });
        self.caught_up(&since, replay)?;

        log::info!("Hermes runs session {}", id.0);
        Ok((id, since))
    }

    async fn prompt(&mut self, id: &SessionId, prompt: &[ContentBlock], responder: Responder<PromptResponse>) -> Result<()> {
        let Some(session) = self.session.as_mut().filter(|session| session.id == *id) else {
            return Ok(responder.respond_with_error(agent_client_protocol::Error::invalid_params().data("no such session"))?);
        };

        let refusal = match &session.link {
            _ if session.prompt.is_some() => Some("a prompt is running".to_owned()),
            Link::Down(_) => Some("Hermes is out of reach; reconnecting".to_owned()),
            Link::Up(gateway) => submit(gateway, &session.live, prompt).await.err().map(|e| e.to_string()),
        };

        match refusal {
            Some(refusal) => responder.respond_with_error(agent_client_protocol::Error::internal_error().data(refusal))?,
            None => {
                session.prompt = Some(responder);
                session.cancelled = false;
            }
        }

        Ok(())
    }

    /// What Hermes sent on the socket. Events and requests of another
    /// session on it are left out.
    fn incoming(&mut self, incoming: Incoming) -> Result<()> {
        let params = match &incoming {
            Incoming::Event(params) | Incoming::Ask { params, .. } => params,
        };
        let live = self.session.as_ref().map(|session| session.live.as_str());
        if params["session_id"].as_str().is_some_and(|id| !id.is_empty() && Some(id) != live) {
            return Ok(());
        }

        match incoming {
            Incoming::Event(params) => self.event(&params),
            Incoming::Ask { id, method, params } => self.ask(id, &method, &params),
        }
    }

    /// An event of Hermes's: what it shows, and the end of the turn.
    fn event(&mut self, params: &Value) -> Result<()> {
        let Some(session) = self.session.as_mut() else {
            return Ok(());
        };

        if let Some(seq) = params["seq"].as_u64() {
            session.seq = session.seq.max(seq);
        }
        if let Some(id) = events::withdrawn(params)
            && let Some(asked) = session.asking.remove(id)
        {
            // Its answer, if the client still gives one, is dropped.
            if let Err(e) = asked.cancel() {
                log::warn!("a withdrawn question stayed open: {e}");
            }
        }
        for update in events::update(params) {
            let notification = json!({"sessionId": session.id, "update": update});
            self.cx.send_notification(UntypedMessage::new(CLIENT_METHOD_NAMES.session_update, notification)?)?;
        }
        if events::finished(params) {
            ended(session, &params["payload"])?;
        }

        Ok(())
    }

    /// Hermes's request `id`: the client is asked, and its answer comes back
    /// through `answers`. One the client is already answering is not asked
    /// twice; one it cannot take is answered at once.
    fn ask(&mut self, id: String, method: &str, params: &Value) -> Result<()> {
        let Some(session) = self.session.as_mut() else {
            return Ok(());
        };

        if session.asking.contains_key(&id) {
            return Ok(());
        }

        let Some((asked, mut request)) = ask::to_client(method, params) else {
            let Link::Up(gateway) = &session.link else {
                return Ok(());
            };
            let sent = match ask::refused(method) {
                Some(result) => gateway.answer(&id, result),
                None => gateway.refuse(&id, format!("{method} is not supported")),
            };
            if let Err(e) = sent {
                log::warn!("Hermes's {method} went unanswered: {e}");
            }
            return Ok(());
        };

        request["sessionId"] = json!(session.id);
        let method = match asked {
            Asked::Approval { .. } => CLIENT_METHOD_NAMES.session_request_permission,
            Asked::Clarify { .. } => CLIENT_METHOD_NAMES.elicitation_create,
        };
        let sent = self.cx.send_request(UntypedMessage::new(method, request)?);
        session.asking.insert(id.clone(), sent.cancellation_handle());
        let sent = sent.block_task();
        self.answers.push(
            async move {
                // A failed request reads as a refusal.
                let reply = sent.await.unwrap_or_default();
                Answer { result: ask::answer(&asked, &reply), id }
            }
            .boxed(),
        );

        Ok(())
    }

    /// Sends the client's answer, or holds it while the socket is down. An
    /// answer to a request Hermes withdrew is dropped.
    fn answered(&mut self, answer: Answer) {
        let Some(session) = self.session.as_mut() else {
            return;
        };

        if session.asking.remove(&answer.id).is_none() {
            return;
        }
        let sent = match &session.link {
            Link::Up(gateway) => gateway.answer(&answer.id, answer.result.clone()),
            Link::Down(_) => Err(LinkError::Closed),
        };
        if sent.is_err() {
            session.held.push(answer);
        }
    }

    /// The socket closed: reconnect and resume from the last event seen.
    fn dropped(&mut self) {
        let Some(session) = self.session.as_mut() else {
            return;
        };

        log::warn!("the link to Hermes closed; reconnecting");
        session.link = Link::Down(reconnect(self.hermes.clone(), session.id.0.to_string(), session.seq).boxed());
    }

    /// Back on a new socket: what was missed is shown, held answers go,
    /// open requests are asked, and a turn cancelled meanwhile is stopped.
    /// A turn is not waited on when Hermes restarted, which ended it.
    async fn resumed(&mut self, resumed: Resumed) -> Result<()> {
        let Some(session) = self.session.as_mut() else {
            return Ok(());
        };

        log::info!("reconnected to Hermes session {}", session.id.0);
        let epoch = &resumed.since["epoch"];
        let restarted = !session.epoch.is_null() && *epoch != session.epoch;
        session.epoch = epoch.clone();
        session.live = resumed.live;
        let mut sent = HashSet::new();
        for held in std::mem::take(&mut session.held) {
            if let Err(e) = resumed.gateway.answer(&held.id, held.result) {
                log::warn!("a held answer to Hermes was lost: {e}");
            }
            sent.insert(held.id);
        }
        let stopped = std::mem::take(&mut session.interrupt);
        if restarted {
            log::warn!("Hermes restarted; its turn ended");
            session.cancelled = true;
            ended(session, &Value::Null)?;
        } else if stopped {
            // The turn already ended for the client; a failure is logged.
            let _ = interrupt(&resumed.gateway, &session.live).await;
        } else if session.cancelled
            && session.prompt.is_some()
            && interrupt(&resumed.gateway, &session.live).await.is_err()
        {
            ended(session, &Value::Null)?;
        }
        session.link = Link::Up(resumed.gateway);

        self.caught_up(&resumed.since, true)?;
        self.ask_open(&resumed.since, &sent)
    }

    /// Shows the events of `since` when `replay`, and takes its latest
    /// sequence as the last seen: after a restart of Hermes the count may
    /// start again.
    fn caught_up(&mut self, since: &Value, replay: bool) -> Result<()> {
        if replay {
            for event in since["events"].as_array().into_iter().flatten() {
                self.event(event)?;
            }
        }

        if let Some((session, latest)) = self.session.as_mut().zip(since["latest_seq"].as_u64()) {
            session.seq = latest;
        }

        Ok(())
    }

    /// Asks the client what Hermes still waits on, as `since` lists it,
    /// but for the requests whose answers were just sent.
    fn ask_open(&mut self, since: &Value, sent: &HashSet<String>) -> Result<()> {
        for open in since["open_requests"].as_array().into_iter().flatten() {
            let (Some(id), Some(method)) = (open["id"].as_str(), open["method"].as_str()) else {
                continue;
            };
            if !sent.contains(id) {
                self.ask(id.to_owned(), method, &open["params"])?;
            }
        }

        Ok(())
    }
}

/// What the link brings next, once a session runs.
async fn next(session: &mut Option<Session>) -> Happened {
    match session.as_mut().map(|session| &mut session.link) {
        Some(Link::Up(gateway)) => match gateway.next().await {
            Some(incoming) => Happened::Incoming(incoming),
            None => Happened::Dropped,
        },
        Some(Link::Down(reconnecting)) => Happened::Resumed(reconnecting.await),
        None => std::future::pending().await,
    }
}

/// Connects again and resumes the stored session `id` from `seq`, a try
/// every `RETRY` with `TRY` for each, for as long as the session lives.
async fn reconnect(hermes: Hermes, id: String, seq: u64) -> Resumed {
    loop {
        let tried = tokio::time::timeout(TRY, async {
            let gateway = Gateway::connect(&hermes.host, hermes.port, &hermes.token).await?;
            let (live, since) = resume_on(&gateway, &id, seq).await?;
            Ok::<_, LinkError>(Resumed { gateway, live, since })
        });

        match tried.await {
            Ok(Ok(resumed)) => return resumed,
            Ok(Err(e)) => log::info!("Hermes is not back yet: {e}"),
            Err(_) => log::info!("Hermes is not back yet: no answer in time"),
        }
        tokio::time::sleep(RETRY).await;
    }
}

/// Resumes the stored session `id` on `gateway`: its live id, and what
/// Hermes sent since `seq` with the requests it still waits on.
async fn resume_on(gateway: &Gateway, id: &str, seq: u64) -> Result<(String, Value), LinkError> {
    let resumed = gateway.call("session.resume", json!({"session_id": id})).await?;
    // The live id is read leniently: kept where Hermes nests it, else the
    // stored id.
    let live = resumed["session_id"].as_str().or(resumed["session"]["session_id"].as_str()).unwrap_or(id).to_owned();
    let since = gateway.call("session.events.since", json!({"session_id": live, "last_seen": seq})).await?;

    Ok((live, since))
}

/// Sends the prompt's images, which Hermes queues for the next turn, then
/// its text.
async fn submit(gateway: &Gateway, live: &str, prompt: &[ContentBlock]) -> Result<(), LinkError> {
    for block in prompt {
        let ContentBlock::Image(image) = block else {
            continue;
        };
        // Hermes takes only strings here, and tells the type by the bytes.
        let ext = image.mime_type.strip_prefix("image/").unwrap_or_default();
        let content = format!("data:{};base64,{}", image.mime_type, image.data);
        let attach = json!({"session_id": live, "content_base64": content, "ext": ext});
        gateway.call("image.attach_bytes", attach).await?;
    }

    gateway.call("prompt.submit", json!({"session_id": live, "text": text(prompt)})).await.map(drop)
}

async fn interrupt(gateway: &Gateway, live: &str) -> Result<(), LinkError> {
    let interrupted = gateway.call("session.interrupt", json!({"session_id": live})).await.map(drop);
    if let Err(e) = &interrupted {
        log::warn!("Hermes did not stop: {e}");
    }

    interrupted
}

/// The turn ended with `payload`: the prompt it ran is answered.
fn ended(session: &mut Session, payload: &Value) -> Result<()> {
    let cancelled = std::mem::take(&mut session.cancelled);
    let Some(responder) = session.prompt.take() else {
        return Ok(());
    };

    let stop = match payload["status"].as_str() {
        _ if cancelled => StopReason::Cancelled,
        Some("interrupted" | "cancelled") => StopReason::Cancelled,
        _ => StopReason::EndTurn,
    };

    Ok(responder.respond(PromptResponse::new(stop))?)
}

/// Fails the session's start with `e`, which ends the bridge.
fn refused<T: JsonRpcResponse>(responder: Responder<T>, e: anyhow::Error) -> Result<()> {
    responder.respond_with_error(agent_client_protocol::Error::internal_error().data(format!("{e:#}")))?;
    Err(e)
}

/// What this agent can do: load and resume Hermes's stored sessions.
fn initialized() -> InitializeResponse {
    InitializeResponse::new(ProtocolVersion::V1)
        .agent_capabilities(
            AgentCapabilities::new()
                .load_session(true)
                .session_capabilities(SessionCapabilities::new().resume(SessionResumeCapabilities::new())),
        )
        .agent_info(Implementation::new("Hermes", env!("CARGO_PKG_VERSION")))
}

/// A prompt as Hermes takes it: its text, with links and embedded files as
/// text. Images go before it, by `submit`.
fn text(prompt: &[ContentBlock]) -> String {
    prompt
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text(said) => Some(said.text.clone()),
            ContentBlock::ResourceLink(link) => Some(format!("[@{}]({})", link.name, link.uri)),
            ContentBlock::Resource(resource) => match &resource.resource {
                EmbeddedResourceResource::TextResourceContents(file) => {
                    Some(format!("<context ref=\"{}\">\n{}\n</context>", file.uri, file.text))
                }
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}
