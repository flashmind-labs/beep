//! The bridge's task: ACP from the client on one side, Codex's app-server on
//! the other. One session, one thread, per process, as the node runs them.

use super::{
    ask, items,
    server::{AppServer, Incoming, TooLong},
    settings::{INITIAL_MODE, Mode, Model, Settings, Started},
};
use crate::agents::{
    acp::{
        self,
        bridge::{CODEX_NAME, Event, Start, notify, parse_start, unknown_session},
    },
    launch::Cli,
};
use crate::machine::wsl::DistroName;
use agent_client_protocol::{
    Client, ConnectionTo, JsonRpcMessage, Responder, UntypedMessage,
    v1::schema::{
        ContentBlock, EmbeddedResourceResource, InitializeRequest, McpServer, Meta, PromptRequest,
        PromptResponse, SessionId, SessionModeState, SessionUpdate, SetSessionConfigOptionRequest,
        SetSessionModeRequest, SetSessionModeResponse, StopReason, SystemPrompt, Usage,
    },
};
use anyhow::{Context, Result};
use futures::{FutureExt, StreamExt, future::BoxFuture, stream::FuturesUnordered};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::{collections::HashSet, path::Path};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::mpsc,
};

/// The command that compacts the thread.
const COMPACT: &str = "/compact";
/// Codex settings the node gives, as a JSON object, for every thread.
const CONFIG: &str = "CODEX_CONFIG";
/// The most pages of a thread's turns a loaded session replays.
const MAX_PAGES: usize = 250;
/// How many turns a page of history asks for; fewer when a page is too
/// long to read.
const PAGE: u64 = 20;

/// Serves ACP read from `input` and written to `output`, driving Codex's
/// CLI `cli`, in the WSL distribution `wsl` when given, until the client
/// closes it or Codex exits; then ends Codex.
pub(in crate::agents) async fn serve(
    input: impl AsyncRead + Unpin + Send + 'static,
    output: impl AsyncWrite + Unpin + Send + 'static,
    cli: Cli,
    wsl: Option<DistroName>,
) -> Result<()> {
    acp::bridge::serve(CODEX_NAME, input, output, async move |cx, mut events| {
        let mut bridge = Bridge {
            cx,
            cli,
            wsl,
            forms: false,
            session: None,
            answers: FuturesUnordered::new(),
        };
        let ran = bridge.run(&mut events).await;

        if let Some(session) = bridge.session.take() {
            session.server.stop().await;
        }

        ran
    })
    .await
}

/// The client's answer to what Codex asked: the request it answers and
/// what Codex is told.
struct Answer {
    id: Value,
    answer: Value,
}

/// A thread's token counts, as `thread/tokenUsage/updated` gives them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Tokens {
    input_tokens: u64,
    cached_input_tokens: u64,
    output_tokens: u64,
    reasoning_output_tokens: u64,
    total_tokens: u64,
}

impl Tokens {
    /// What was used since `before`.
    fn since(self, before: Self) -> Self {
        Self {
            input_tokens: self.input_tokens.saturating_sub(before.input_tokens),
            cached_input_tokens: self
                .cached_input_tokens
                .saturating_sub(before.cached_input_tokens),
            output_tokens: self.output_tokens.saturating_sub(before.output_tokens),
            reasoning_output_tokens: self
                .reasoning_output_tokens
                .saturating_sub(before.reasoning_output_tokens),
            total_tokens: self.total_tokens.saturating_sub(before.total_tokens),
        }
    }
}

struct Session {
    id: SessionId,
    server: AppServer,
    settings: Settings,
    /// The prompt that runs, until its turn ends.
    running: Option<Running>,
    /// The turn `turn/started` named while no prompt ran, which the prompt
    /// about to start may take when Codex's answer names none.
    started: Option<String>,
    /// Items whose text came as deltas, which their completion leaves out.
    streamed: HashSet<String>,
    /// The thread's tokens so far.
    tokens: Tokens,
}

/// A prompt Codex runs, with the turn it started.
struct Running {
    prompt: Responder<PromptResponse>,
    /// The turn Codex runs.
    turn: Option<String>,
    cancelled: bool,
    /// The thread's tokens when the prompt started.
    prompted_at: Tokens,
    /// Why the turn failed, as the last error Codex gave up on says.
    failure: Option<String>,
}

struct Bridge {
    cx: ConnectionTo<Client>,
    cli: Cli,
    /// The WSL distribution Codex runs in, on a Windows machine.
    wsl: Option<DistroName>,
    /// The client takes forms, for Codex's questions.
    forms: bool,
    session: Option<Session>,
    answers: FuturesUnordered<BoxFuture<'static, Answer>>,
}

impl Bridge {
    async fn run(&mut self, events: &mut mpsc::Receiver<Event>) -> Result<()> {
        loop {
            tokio::select! {
                event = events.recv() => match event {
                    Some(Event::Request(message, responder)) => self.request(message, *responder).await?,
                    Some(Event::Notification(message)) => self.notification(&message).await,
                    None => return Ok(()),
                },
                () = self.cx.incoming_closed() => return Ok(()),
                incoming = next(&mut self.session) => match incoming {
                    Some(Incoming::Notification { method, params }) => self.notified(&method, params)?,
                    Some(Incoming::Request { id, method, params }) => self.asked(id, &method, &params),
                    None => anyhow::bail!("Codex exited"),
                },
                Some(answer) = self.answers.next() => {
                    if let Some(session) = &self.session {
                        session.server.respond(answer.id, answer.answer);
                    }
                }
            }
        }
    }

    async fn request(&mut self, message: UntypedMessage, responder: Responder) -> Result<()> {
        let (method, params) = (message.method(), message.params());

        match method {
            "initialize" => {
                let request = InitializeRequest::parse_message(method, params)?;
                self.forms = acp::bridge::accepts_forms(&request);
                responder
                    .cast()
                    .respond(acp::bridge::initialized(CODEX_NAME, false, true))?;
            }
            "session/new" | "session/load" | "session/resume" => {
                let req = parse_start(&message, responder)?;
                self.start(
                    req.start,
                    req.session_id.as_deref(),
                    &req.cwd,
                    &req.mcp_servers,
                    req.system_prompt.as_ref(),
                )
                .await?;
            }
            "session/prompt" => {
                let request = PromptRequest::parse_message(method, params)?;
                self.prompt(&request.session_id, &request.prompt, responder.cast())
                    .await?;
            }
            "_session/steering" => {
                let outcome = self.steer(params).await;
                responder.respond(json!({"outcome": outcome}))?;
            }
            "session/set_mode" => {
                let request = SetSessionModeRequest::parse_message(method, params)?;
                let responder = responder.cast::<SetSessionModeResponse>();
                let Some(session) = self.session.as_mut().filter(|s| s.id == request.session_id)
                else {
                    return Ok(responder.respond_with_error(unknown_session())?);
                };

                match session.settings.set_mode(&request.mode_id.to_string()) {
                    Ok(()) => responder.respond(SetSessionModeResponse::new())?,
                    Err(e) => responder.respond_with_error(
                        agent_client_protocol::Error::invalid_params().data(e),
                    )?,
                }
            }
            "session/set_config_option" => {
                let request = SetSessionConfigOptionRequest::parse_message(method, params)?;
                let Some(session) = self.session.as_mut().filter(|s| s.id == request.session_id)
                else {
                    return Ok(responder.respond_with_error(unknown_session())?);
                };
                let value = request
                    .value
                    .as_value_id()
                    .map(|id| id.0.to_string())
                    .unwrap_or_default();

                match session.settings.set(&request.config_id.0, &value) {
                    Ok(()) => {
                        responder.respond(json!({"configOptions": session.settings.options()}))?
                    }
                    Err(e) => responder.respond_with_error(
                        agent_client_protocol::Error::invalid_params().data(e),
                    )?,
                }
            }
            _ => responder.respond_with_error(agent_client_protocol::Error::method_not_found())?,
        }

        Ok(())
    }

    async fn notification(&mut self, message: &UntypedMessage) {
        let Some(cancel_id) = acp::bridge::cancel_session_id(message) else {
            return;
        };
        let Some(session) = self
            .session
            .as_mut()
            .filter(|session| session.id == cancel_id)
        else {
            return;
        };
        let Some(running) = session
            .running
            .as_mut()
            .filter(|running| !running.cancelled)
        else {
            return;
        };
        let Some(turn) = running.turn.clone() else {
            return;
        };

        running.cancelled = true;
        let interrupt = json!({"threadId": session.id, "turnId": turn});

        if let Err(e) = session.server.call("turn/interrupt", interrupt).await {
            log::warn!("Codex did not stop: {e:#}");
        }
    }

    /// Starts Codex on a new thread, or on `thread` resumed, in `cwd`;
    /// `start` is answered once the thread runs.
    async fn start(
        &mut self,
        start: Start,
        thread: Option<&str>,
        cwd: &Path,
        mcp: &[McpServer],
        system: Option<&SystemPrompt>,
    ) -> Result<()> {
        let Some(start) = start.check(self.session.is_some(), system)? else {
            return Ok(());
        };

        let replay = start.is_load();
        let opened = self.open(thread, cwd, mcp, system, replay).await;
        let (session, replayed) = match opened {
            Ok(opened) => opened,
            Err(e) => {
                start.refuse(
                    agent_client_protocol::Error::internal_error().data(format!("{e:#}")),
                )?;
                return Err(e);
            }
        };
        let modes: SessionModeState = serde_json::from_value(session.settings.modes())?;
        let options: Vec<_> = serde_json::from_value(session.settings.options())?;
        start.respond(&self.cx, &session.id, modes, options, replayed)?;

        // The node asks for compaction with the command it advertises.
        let commands = json!({
            "sessionUpdate": "available_commands_update",
            "availableCommands": [{"name": COMPACT.trim_start_matches('/'), "description": "Summarize the conversation to free context"}],
        });

        if let Some(update) = items::update(commands) {
            notify(&self.cx, &session.id, update)?;
        }

        log::info!("Codex runs thread {} in {}", session.id.0, cwd.display());
        self.session = Some(session);
        Ok(())
    }

    /// Starts the app-server and its thread, with the updates that replay
    /// its history when `replay`.
    async fn open(
        &mut self,
        thread: Option<&str>,
        cwd: &Path,
        mcp: &[McpServer],
        system: Option<&SystemPrompt>,
        replay: bool,
    ) -> Result<(Session, Vec<SessionUpdate>)> {
        let mut server = AppServer::start(&mut self.cli, cwd, self.wsl.as_ref()).await?;
        let mode = Mode::initial(self.cli.var(INITIAL_MODE).as_deref());
        let mut params = mode.thread();
        let config = config(self.cli.var(CONFIG), mcp, self.wsl.is_some())?;
        params.insert("cwd".to_owned(), json!(cwd));
        params.insert("config".to_owned(), Value::Object(config));

        if let Some(system) = acp::bridge::system_text(system) {
            let instructions = developer_instructions(&mut server, cwd, &system).await;
            params.insert("developerInstructions".to_owned(), json!(instructions));
        }

        let started = match thread {
            Some(thread) => {
                params.insert("threadId".to_owned(), json!(thread));
                params.insert("excludeTurns".to_owned(), json!(true));
                server.call("thread/resume", Value::Object(params)).await?
            }
            None => server.call("thread/start", Value::Object(params)).await?,
        };
        let id = started["thread"]["id"]
            .as_str()
            .context("Codex started no thread")?
            .to_owned();
        let models = models(&mut server).await;
        let settings = Settings::new(
            mode,
            models,
            Started::deserialize(&started).unwrap_or_default(),
        );
        let replayed = match replay {
            true => history(&mut server, &id).await,
            false => Vec::new(),
        };

        let session = Session {
            id: SessionId::new(id),
            server,
            settings,
            running: None,
            started: None,
            streamed: HashSet::new(),
            tokens: Tokens::default(),
        };
        Ok((session, replayed))
    }

    async fn prompt(
        &mut self,
        id: &SessionId,
        prompt: &[ContentBlock],
        responder: Responder<PromptResponse>,
    ) -> Result<()> {
        let Some(session) = self.session.as_mut().filter(|session| session.id == *id) else {
            return Ok(responder.respond_with_error(unknown_session())?);
        };

        if session.running.is_some() {
            return Ok(responder.respond_with_error(acp::bridge::prompt_running())?);
        }

        // `/compact` runs as a turn of its own.
        let compact = matches!(prompt, [ContentBlock::Text(said)] if said.text.trim() == COMPACT);
        let called = match compact {
            true => {
                session
                    .server
                    .call("thread/compact/start", json!({"threadId": session.id}))
                    .await
            }
            false => {
                let mut params = session.settings.turn();
                params.insert("threadId".to_owned(), json!(session.id));
                params.insert("input".to_owned(), json!(input(prompt)));
                session
                    .server
                    .call("turn/start", Value::Object(params))
                    .await
            }
        };

        match called {
            Ok(started) => {
                session.running = Some(Running {
                    prompt: responder,
                    turn: started_turn(&started, &mut session.started),
                    cancelled: false,
                    prompted_at: session.tokens,
                    failure: None,
                });
            }
            Err(e) => responder.respond_with_error(
                agent_client_protocol::Error::internal_error().data(format!("{e:#}")),
            )?,
        }

        Ok(())
    }

    /// Puts a prompt into the running turn: `injected`, or `promptRequired`
    /// when no turn runs and the client sends it as a prompt of its own.
    async fn steer(&mut self, params: &Value) -> &'static str {
        let Some(session) = self
            .session
            .as_mut()
            .filter(|session| session.id.0.as_ref() == params["sessionId"])
        else {
            return "failed";
        };
        let Some(turn) = session
            .running
            .as_ref()
            .and_then(|running| running.turn.clone())
        else {
            return "promptRequired";
        };
        let Ok(prompt) = Vec::<ContentBlock>::deserialize(&params["prompt"]) else {
            return "failed";
        };

        let steer =
            json!({"threadId": session.id, "expectedTurnId": turn, "input": input(&prompt)});

        match session.server.call("turn/steer", steer).await {
            Ok(_) => "injected",
            // The turn ended meanwhile, though its end may not be read yet.
            Err(_)
                if session
                    .running
                    .as_ref()
                    .is_none_or(|running| running.turn.as_ref() != Some(&turn))
                    || session.server.sent("turn/completed") =>
            {
                "promptRequired"
            }
            Err(e) => {
                log::warn!("Codex took no steer: {e:#}");
                "failed"
            }
        }
    }

    /// Codex's notification `method`: what it shows of the session's own
    /// thread. A subagent's thread is read from its log.
    fn notified(&mut self, method: &str, params: Value) -> Result<()> {
        let Some(session) = self.session.as_mut() else {
            return Ok(());
        };

        if params["threadId"]
            .as_str()
            .is_some_and(|thread| thread != session.id.0.as_ref())
        {
            return Ok(());
        }

        let item_id = || params["itemId"].as_str().unwrap_or_default().to_owned();
        let update = match method {
            "item/started" => items::started(&params["item"]),
            "item/completed" => {
                let item = &params["item"];
                let whole = item["id"]
                    .as_str()
                    .is_none_or(|id| !session.streamed.remove(id));
                items::completed(item, whole)
            }
            "item/agentMessage/delta" | "item/plan/delta" => {
                session.streamed.insert(item_id());
                Some(items::chunk(
                    "agent_message_chunk",
                    params["delta"].as_str().unwrap_or_default(),
                ))
            }
            "item/reasoning/summaryTextDelta" | "item/reasoning/textDelta" => {
                session.streamed.insert(item_id());
                Some(items::chunk(
                    "agent_thought_chunk",
                    params["delta"].as_str().unwrap_or_default(),
                ))
            }
            "item/reasoning/summaryPartAdded" => {
                session.streamed.insert(item_id());
                Some(items::chunk("agent_thought_chunk", "\n\n"))
            }
            "turn/plan/updated" => Some(items::plan(&params)),
            "thread/tokenUsage/updated" => {
                let usage = &params["tokenUsage"];
                session.tokens = Tokens::deserialize(&usage["total"]).unwrap_or(session.tokens);
                let used = usage["last"]["totalTokens"].as_u64();
                let size = usage["modelContextWindow"]
                    .as_u64()
                    .filter(|size| *size > 0);
                used.zip(size).map(|(used, size)| {
                    json!({"sessionUpdate": "usage_update", "used": used, "size": size})
                })
            }
            "thread/name/updated" => {
                Some(json!({"sessionUpdate": "session_info_update", "title": params["threadName"]}))
            }
            "model/rerouted" => Some(items::chunk(
                "agent_thought_chunk",
                &format!(
                    "Model rerouted from {} to {} ({}).\n\n",
                    text(&params, "fromModel"),
                    text(&params, "toModel"),
                    params["reason"].as_str().unwrap_or("rerouted"),
                ),
            )),
            "warning" => Some(items::chunk(
                "agent_message_chunk",
                &format!("Warning: {}\n\n", text(&params, "message")),
            )),
            "error" => {
                if params["willRetry"] != true
                    && let Some(running) = session.running.as_mut()
                {
                    running.failure = params["error"]["message"].as_str().map(str::to_owned);
                }

                None
            }
            "turn/started" => {
                let turn = params["turn"]["id"].as_str().map(str::to_owned);

                match session.running.as_mut() {
                    Some(running) => running.turn = turn,
                    None => session.started = turn,
                }

                None
            }
            "turn/completed" => return ended(session, &params["turn"]),
            _ => None,
        };

        if let Some(update) = update.and_then(items::update) {
            notify(&self.cx, &session.id, update)?;
        }

        Ok(())
    }

    /// Codex's request `method`: the client is asked, and its answer comes
    /// back through `answers`.
    fn asked(&mut self, id: Value, method: &str, params: &Value) {
        let Some(session) = &self.session else {
            return;
        };

        let Some((request, asked)) = ask::ask(&session.id, method, params, self.forms) else {
            match ask::refused(method) {
                Value::Null => session
                    .server
                    .refuse(id, &format!("{method} is not supported")),
                answer => session.server.respond(id, answer),
            }

            return;
        };

        let reply = acp::bridge::ask_client(&self.cx, request);
        self.answers.push(
            async move {
                let answer = ask::answer(asked, reply.await);
                Answer { id, answer }
            }
            .boxed(),
        );
    }
}

/// The turn a `turn/start` answer names, else the one `turn/started` named
/// before it.
fn started_turn(answer: &Value, seen: &mut Option<String>) -> Option<String> {
    answer["turn"]["id"]
        .as_str()
        .map(str::to_owned)
        .or(seen.take())
}

/// The turn `turn` ended: the prompt it ran is answered.
fn ended(session: &mut Session, turn: &Value) -> Result<()> {
    session.streamed.clear();
    session.started = None;
    let Some(Running {
        prompt: responder,
        cancelled,
        prompted_at,
        failure,
        ..
    }) = session.running.take()
    else {
        return Ok(());
    };

    let used = session.tokens.since(prompted_at);
    let stop = match turn["status"].as_str() {
        _ if cancelled => StopReason::Cancelled,
        Some("interrupted") => StopReason::Cancelled,
        Some("failed") => {
            let error = turn["error"]["message"]
                .as_str()
                .map(str::to_owned)
                .or(failure);
            let error = error.unwrap_or_else(|| "Codex turn failed.".to_owned());
            return Ok(responder
                .respond_with_error(agent_client_protocol::Error::internal_error().data(error))?);
        }
        _ => StopReason::EndTurn,
    };

    let mut response = PromptResponse::new(stop);

    if used.total_tokens > 0 {
        let usage = Usage::new(used.total_tokens, used.input_tokens, used.output_tokens)
            .cached_read_tokens(used.cached_input_tokens)
            .thought_tokens(used.reasoning_output_tokens);
        let count = json!({
            "inputTokens": used.input_tokens,
            "cachedInputTokens": used.cached_input_tokens,
            "outputTokens": used.output_tokens,
            "reasoningOutputTokens": used.reasoning_output_tokens,
            "totalTokens": used.total_tokens,
        });
        let mut meta = Meta::new();
        meta.insert(
            "quota".to_owned(),
            json!({"token_count": count, "model_usage": [{"model": session.settings.model(), "token_count": count}]}),
        );
        response = response.usage(usage).meta(meta);
    }

    responder.respond(response)?;
    Ok(())
}

/// The settings a thread starts with: `base`, the session's `CODEX_CONFIG`,
/// and the MCP servers. In WSL a server on stdio is a Windows program: it
/// is named by its path inside the distribution, and its variables cross
/// over in `WSLENV`.
fn config(base: Option<String>, servers: &[McpServer], in_wsl: bool) -> Result<Map<String, Value>> {
    let mut config = match base.filter(|config| !config.trim().is_empty()) {
        Some(config) => serde_json::from_str(&config)
            .with_context(|| format!("{CONFIG} must be a JSON object"))?,
        None => Map::new(),
    };
    let mut servers_config = Map::new();

    for server in servers {
        let (name, entry) = match server {
            McpServer::Stdio(stdio) => (&stdio.name, acp::bridge::mcp_stdio(stdio, in_wsl)),
            McpServer::Http(http) => (
                &http.name,
                json!({
                    "url": http.url,
                    "http_headers": acp::bridge::mcp_headers(&http.headers),
                }),
            ),
            _ => continue,
        };
        servers_config.insert(name.clone(), entry);
    }

    if !servers_config.is_empty() {
        config.insert("mcp_servers".to_owned(), Value::Object(servers_config));
    }

    Ok(config)
}

/// `system` after the developer instructions the user set for `cwd`, which
/// Codex would otherwise replace with it.
async fn developer_instructions(server: &mut AppServer, cwd: &Path, system: &str) -> String {
    let read = server
        .call("config/read", json!({"cwd": cwd, "includeLayers": false}))
        .await;
    let baseline = read
        .inspect_err(|e| log::warn!("Codex's settings were not read: {e:#}"))
        .ok()
        .and_then(|read| {
            read["config"]["developer_instructions"]
                .as_str()
                .map(str::to_owned)
        })
        .filter(|baseline| !baseline.is_empty());

    match baseline {
        Some(baseline) => format!("{baseline}\n\n{system}"),
        None => system.to_owned(),
    }
}

/// The models Codex offers, page by page; none when it does not say.
async fn models(server: &mut AppServer) -> Vec<Model> {
    let mut models = Vec::new();
    let mut cursor = Value::Null;

    for _ in 0..MAX_PAGES {
        let page = match server
            .call(
                "model/list",
                json!({"cursor": cursor, "includeHidden": false}),
            )
            .await
        {
            Ok(page) => page,
            Err(e) => {
                log::warn!("Codex listed no models: {e:#}");
                break;
            }
        };
        let listed = page["data"].as_array().into_iter().flatten();
        models.extend(
            listed
                .filter_map(|model| Model::deserialize(model).ok())
                .take(acp::MAX_MODELS),
        );
        cursor = page["nextCursor"].clone();

        if cursor.is_null() || models.len() >= acp::MAX_MODELS {
            break;
        }
    }

    models
}

/// The updates replaying thread `id`'s turns, oldest first; what was read
/// when Codex stops answering.
async fn history(server: &mut AppServer, id: &str) -> Vec<SessionUpdate> {
    let mut updates = Vec::new();
    let mut cursor = Value::Null;
    let mut limit = PAGE;

    for _ in 0..MAX_PAGES {
        let params = json!({"threadId": id, "cursor": cursor, "sortDirection": "asc", "itemsView": "full", "limit": limit});
        let page = match server.call("thread/turns/list", params).await {
            Ok(page) => page,
            // Images and long tool output can make a page too long: the
            // same turns are asked for in smaller pages.
            Err(e) if e.is::<TooLong>() && limit > 1 => {
                limit /= 2;
                continue;
            }
            Err(e) => {
                log::warn!("the history of {id} was not replayed: {e:#}");
                break;
            }
        };
        let turns = page["data"].as_array().into_iter().flatten();
        let items = turns.flat_map(|turn| turn["items"].as_array().into_iter().flatten());
        updates.extend(items.flat_map(items::replayed).filter_map(items::update));
        cursor = page["nextCursor"].clone();

        if cursor.is_null() {
            break;
        }
    }

    updates
}

/// What Codex sends next, once a session runs.
async fn next(session: &mut Option<Session>) -> Option<Incoming> {
    match session {
        Some(session) => session.server.next().await,
        None => std::future::pending().await,
    }
}

/// A prompt as Codex takes it, as Codex's adapter turns it: text, images by
/// URL, and links and embedded files as text.
fn input(prompt: &[ContentBlock]) -> Vec<Value> {
    let text = |text: String| json!({"type": "text", "text": text, "text_elements": []});
    prompt
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text(said) => Some(text(said.text.clone())),
            ContentBlock::Image(image) => {
                let url = image
                    .uri
                    .as_deref()
                    .filter(|uri| {
                        ["http:", "https:", "data:"]
                            .iter()
                            .any(|scheme| uri.starts_with(scheme))
                    })
                    .map_or_else(
                        || format!("data:{};base64,{}", image.mime_type, image.data),
                        str::to_owned,
                    );
                Some(json!({"type": "image", "url": url}))
            }
            ContentBlock::ResourceLink(link) => Some(text(match link.name.is_empty() {
                true => link.uri.clone(),
                false => format!("[@{}]({})", link.name, link.uri),
            })),
            ContentBlock::Resource(resource) => match &resource.resource {
                EmbeddedResourceResource::TextResourceContents(file) => Some(text(format!(
                    "{}\n<context ref=\"{}\">\n{}\n</context>",
                    file.uri, file.uri, file.text
                ))),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or_default()
}

#[cfg(test)]
#[path = "../../tests/agents_codex_direct_bridge.rs"]
mod tests;
