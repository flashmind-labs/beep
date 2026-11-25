//! The bridge's task: ACP from the client on one side, Claude Code's CLI on
//! the other. One session per process, as the node runs them.

use super::{
    ask,
    cli::{Cli, Launch, disallowed_tools},
    updates,
};
use crate::agents::{
    acp::{
        self,
        bridge::{ClientReply, ClientRequest, Event, Start, notify, parse_start, unknown_session},
    },
    launch,
    pipe::Frame,
};
use crate::machine::wsl::{self, DistroName, LinuxPath};
use agent_client_protocol::{
    Client, ConnectionTo, JsonRpcMessage, Responder, UntypedMessage,
    v1::schema::{
        AvailableCommand, AvailableCommandInput, AvailableCommandsUpdate, ContentBlock,
        CurrentModeUpdate, InitializeRequest, McpServer, PromptRequest, PromptResponse,
        RequestPermissionRequest, SessionConfigOption, SessionConfigOptionCategory,
        SessionConfigSelectOption, SessionId, SessionMode, SessionModeState, SessionUpdate,
        SetSessionConfigOptionRequest, SetSessionConfigOptionResponse, SetSessionModeRequest,
        SetSessionModeResponse, SystemPrompt, UnstructuredCommandInput,
    },
};
use anyhow::Result;
use futures::{FutureExt, StreamExt, future::BoxFuture, stream::FuturesUnordered};
use serde_json::{Map, Value, json};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::mpsc,
    task::spawn_blocking,
};

const NAME: &str = "beep-node claude-acp";

/// The session modes, as Claude Code names its permission modes.
const MODES: &[(&str, &str)] = &[
    ("default", "Default"),
    ("acceptEdits", "Accept Edits"),
    ("plan", "Plan Mode"),
    ("bypassPermissions", "Bypass Permissions"),
];

/// The config option that picks the model.
const MODEL: &str = "model";

/// Serves ACP read from `input` and written to `output`, driving Claude
/// Code's CLI `cli`, in the WSL distribution `wsl` whose user's home is
/// `home` when given, until the client closes it or the CLI exits; then
/// ends the CLI.
pub(in crate::agents) async fn serve(
    input: impl AsyncRead + Unpin + Send + 'static,
    output: impl AsyncWrite + Unpin + Send + 'static,
    cli: launch::Cli,
    wsl: Option<DistroName>,
    home: Option<LinuxPath>,
) -> Result<()> {
    acp::bridge::serve(NAME, input, output, async move |cx, mut events| {
        let mut bridge = Bridge {
            cx,
            cli,
            wsl,
            home,
            asks: false,
            session: None,
            answers: FuturesUnordered::new(),
        };
        let ran = bridge.run(&mut events).await;

        if let Some(session) = bridge.session.take() {
            session.cli.pipe.stop().await;
        }

        ran
    })
    .await
}

/// A control request of ours the CLI has yet to answer, and what waits on it.
enum Control {
    Initialize(Start),
    Mode {
        mode: String,
        responder: Responder<SetSessionModeResponse>,
    },
    Model {
        model: String,
        responder: Responder<SetSessionConfigOptionResponse>,
    },
    Interrupt,
}

/// A `can_use_tool` request put to the client, by what it asks.
enum Asked {
    Tool {
        input: Value,
        suggested: Option<Value>,
    },
    Plan {
        input: Value,
    },
    Questions {
        input: Value,
        questions: Vec<ask::Question>,
    },
}

/// The client's answer to a `can_use_tool` request: the CLI's request id, and
/// what the CLI is told with the mode the session went on in.
struct Answer {
    request: String,
    decision: Value,
    mode: Option<&'static str>,
}

struct Session {
    id: SessionId,
    cli: Cli,
    mode: String,
    /// The mode it was in before plan mode.
    before_plan: String,
    models: Vec<SessionConfigSelectOption>,
    model: String,
    controls: HashMap<String, Control>,
    /// The prompt being answered, from its request to its result.
    turn: Option<Turn>,
    /// How much of the window the main agent's context fills.
    context: u64,
    /// The messages of this turn whose text came as stream deltas.
    streamed: HashSet<String>,
    /// The main agent's message streaming now.
    message: Option<String>,
}

/// A prompt waiting for Claude Code's result.
struct Turn {
    responder: Responder<PromptResponse>,
    /// An interrupt was sent for it.
    cancelled: bool,
}

impl Session {
    /// Takes `mode` as the one the CLI runs in, remembering the one before
    /// plan mode, which approving the plan returns to.
    fn moved_to(&mut self, mode: &str) {
        if mode == "plan" && self.mode != "plan" {
            self.before_plan = std::mem::take(&mut self.mode);
        }

        self.mode = mode.to_owned();
    }
}

struct Bridge {
    cx: ConnectionTo<Client>,
    cli: launch::Cli,
    /// The WSL distribution the CLI runs in, on a Windows machine.
    wsl: Option<DistroName>,
    /// Its default user's home.
    home: Option<LinuxPath>,
    /// The client takes forms, for `AskUserQuestion`.
    asks: bool,
    session: Option<Session>,
    answers: FuturesUnordered<BoxFuture<'static, Answer>>,
}

impl Bridge {
    async fn run(&mut self, events: &mut mpsc::Receiver<Event>) -> Result<()> {
        loop {
            tokio::select! {
                event = events.recv() => match event {
                    Some(Event::Request(message, responder)) => self.request(message, *responder).await?,
                    Some(Event::Notification(message)) => self.notification(&message),
                    None => return Ok(()),
                },
                () = self.cx.incoming_closed() => return Ok(()),
                frame = next_frame(&mut self.session) => match frame {
                    Some(frame) => self.frame(frame)?,
                    None => anyhow::bail!("Claude Code exited"),
                },
                Some(answer) = self.answers.next() => {
                    if let Some(session) = &mut self.session {
                        session.cli.respond(&answer.request, answer.decision);

                        if let Some(mode) = answer.mode {
                            session.moved_to(mode);
                            notify(&self.cx, &session.id, SessionUpdate::CurrentModeUpdate(CurrentModeUpdate::new(mode)))?;
                        }
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
                self.asks = acp::bridge::accepts_forms(&request);
                responder
                    .cast()
                    .respond(acp::bridge::initialized(NAME, true, false))?;
            }
            "session/new" | "session/load" | "session/resume" => {
                let mut req = parse_start(&message, responder)?;
                let id = req
                    .session_id
                    .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

                if let Start::Load(_, replay) = &mut req.start {
                    let transcript = self.transcript(&req.cwd, &id);
                    let (session, in_wsl) = (id.clone(), self.wsl.is_some());
                    *replay =
                        spawn_blocking(move || replayed(transcript, &session, in_wsl)).await?;
                }

                self.start(
                    &id,
                    &req.cwd,
                    &req.mcp_servers,
                    req.system_prompt.as_ref(),
                    req.meta.as_ref(),
                    req.start,
                )?;
            }
            "session/prompt" => {
                let request = PromptRequest::parse_message(method, params)?;
                let responder = responder.cast();
                let Some(session) = self.session.as_mut().filter(|s| s.id == request.session_id)
                else {
                    return Ok(responder.respond_with_error(unknown_session())?);
                };

                if session.turn.is_some() {
                    return Ok(responder.respond_with_error(acp::bridge::prompt_running())?);
                }

                session
                    .cli
                    .user(request.prompt.iter().filter_map(content).collect());
                session.turn = Some(Turn {
                    responder,
                    cancelled: false,
                });
            }
            "session/set_mode" => {
                let request = SetSessionModeRequest::parse_message(method, params)?;
                let responder = responder.cast();
                let mode = request.mode_id.to_string();
                let Some(session) = self.session.as_mut().filter(|s| s.id == request.session_id)
                else {
                    return Ok(responder.respond_with_error(unknown_session())?);
                };

                if !MODES.iter().any(|(id, _)| *id == mode) {
                    return Ok(responder.respond_with_error(
                        agent_client_protocol::Error::invalid_params()
                            .data(format!("no mode {mode}")),
                    )?);
                }

                let id = session
                    .cli
                    .control(json!({"subtype": "set_permission_mode", "mode": mode}));
                session
                    .controls
                    .insert(id, Control::Mode { mode, responder });
            }
            "session/set_config_option" => {
                let request = SetSessionConfigOptionRequest::parse_message(method, params)?;
                let responder = responder.cast();
                let Some(session) = self.session.as_mut().filter(|s| s.id == request.session_id)
                else {
                    return Ok(responder.respond_with_error(unknown_session())?);
                };
                let model = request
                    .value
                    .as_value_id()
                    .map(|id| id.0.to_string())
                    .filter(|_| request.config_id.0.as_ref() == MODEL);
                let Some(model) = model else {
                    return Ok(responder.respond_with_error(
                        agent_client_protocol::Error::invalid_params()
                            .data("only the model can be set"),
                    )?);
                };

                let id = session
                    .cli
                    .control(json!({"subtype": "set_model", "model": model}));
                session
                    .controls
                    .insert(id, Control::Model { model, responder });
            }
            _ => responder.respond_with_error(agent_client_protocol::Error::method_not_found())?,
        }

        Ok(())
    }

    fn notification(&mut self, message: &UntypedMessage) {
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

        if let Some(turn) = session.turn.as_mut()
            && !turn.cancelled
        {
            turn.cancelled = true;
            let id = session.cli.control(json!({"subtype": "interrupt"}));
            session.controls.insert(id, Control::Interrupt);
        }
    }

    /// Where Claude Code keeps session `id` it ran in `cwd`: under the
    /// user's home, or in WSL the distribution's.
    fn transcript(&self, cwd: &Path, id: &str) -> Option<PathBuf> {
        let Some(distro) = &self.wsl else {
            return updates::transcript(cwd, id);
        };
        let cwd = wsl::linux_path(cwd)?;
        let folder = crate::agents::claude_dir::slug(Path::new(cwd.as_ref()));
        let transcript = self
            .home
            .as_ref()?
            .join(&format!(".claude/projects/{folder}/{id}.jsonl"));
        Some(wsl::windows_path(distro, &transcript))
    }

    /// Starts the CLI on session `id` in `cwd`, new or resumed as `start`
    /// says; `start` is answered once it initialized.
    fn start(
        &mut self,
        id: &str,
        cwd: &Path,
        mcp: &[McpServer],
        system: Option<&SystemPrompt>,
        meta: Option<&Map<String, Value>>,
        start: Start,
    ) -> Result<()> {
        let Some(start) = start.check(self.session.is_some(), system)? else {
            return Ok(());
        };

        let launch = Launch {
            cwd,
            session: id,
            resume: !start.is_new(),
            wsl: self.wsl.as_ref(),
            mcp: mcp_config(mcp, self.wsl.is_some()),
            system: acp::bridge::system_text(system),
            asks: self.asks,
            disallowed: disallowed_tools(meta),
        };
        let mut cli = Cli::start(&mut self.cli, &launch)?;
        let request = cli.control(json!({"subtype": "initialize"}));
        let session = self.session.insert(Session {
            id: SessionId::new(id.to_owned()),
            cli,
            mode: "default".to_owned(),
            before_plan: "default".to_owned(),
            models: Vec::new(),
            model: String::new(),
            controls: HashMap::new(),
            turn: None,
            context: 0,
            streamed: HashSet::new(),
            message: None,
        });
        session.controls.insert(request, Control::Initialize(start));
        log::info!("Claude Code started on session {id} in {}", cwd.display());
        Ok(())
    }

    fn frame(&mut self, frame: Value) -> Result<()> {
        let Some(session) = self.session.as_mut() else {
            return Ok(());
        };
        let cx = &self.cx;

        match frame["type"].as_str() {
            Some("control_response") => {
                let response = &frame["response"];
                let id = response["request_id"].as_str().unwrap_or_default();

                if let Some(control) = session.controls.remove(id) {
                    let answer = match response["subtype"].as_str() {
                        Some("success") => Ok(&response["response"]),
                        _ => Err(response["error"]
                            .as_str()
                            .unwrap_or("Claude Code refused")
                            .to_owned()),
                    };
                    controlled(cx, session, control, answer)?;
                }
            }
            Some("control_request") => self.can_use_tool(&frame)?,
            Some("stream_event") => {
                if let Some(id) = frame["event"]["message"]["id"].as_str() {
                    session.streamed.insert(id.to_owned());

                    if frame["parent_tool_use_id"].is_null() {
                        session.message = Some(id.to_owned());
                    }
                }

                if let Some(update) = updates::delta(&frame, session.message.as_deref()) {
                    notify(cx, &session.id, update)?;
                }
            }
            Some("assistant" | "user") => {
                if frame["type"] == "assistant"
                    && let Some(used) = updates::context(&frame)
                {
                    session.context = used;
                }

                // Text that never streamed, such as a local command's output,
                // shows from the message itself.
                let unstreamed = frame["type"] == "assistant"
                    && frame["message"]["id"]
                        .as_str()
                        .is_none_or(|id| !session.streamed.contains(id));

                for update in updates::message(&frame, unstreamed) {
                    notify(cx, &session.id, update)?;
                }
            }
            // The mode the CLI runs in, which the model can change mid-turn
            // (EnterPlanMode): `init` starts a turn, `status` reports a change.
            Some("system") if matches!(frame["subtype"].as_str(), Some("init" | "status")) => {
                if let Some(mode) = frame["permissionMode"].as_str()
                    && mode != session.mode
                    && MODES.iter().any(|(id, _)| *id == mode)
                {
                    session.moved_to(mode);
                    notify(
                        cx,
                        &session.id,
                        SessionUpdate::CurrentModeUpdate(CurrentModeUpdate::new(mode.to_owned())),
                    )?;
                }
            }
            Some("result") if session.turn.is_some() && updates::ends_prompt(&frame) => {
                if let Some(usage) = updates::usage_update(&frame, session.context) {
                    notify(cx, &session.id, SessionUpdate::UsageUpdate(usage))?;
                }

                session.streamed.clear();

                if let Some(Turn { responder, cancelled }) = session.turn.take() {
                    match updates::stop(&frame, cancelled) {
                        Ok(stop) => responder.respond(
                            PromptResponse::new(stop).usage(updates::turn_usage(&frame)),
                        )?,
                        Err(error) => responder.respond_with_error(
                            agent_client_protocol::Error::internal_error().data(error),
                        )?,
                    }
                }
            }
            _ => {}
        }

        Ok(())
    }

    /// The CLI asks whether a tool may run: the client is asked, and its
    /// answer comes back through `answers`.
    fn can_use_tool(&mut self, frame: &Value) -> Result<()> {
        let Some(session) = self.session.as_mut() else {
            return Ok(());
        };
        let request = frame["request_id"].as_str().unwrap_or_default().to_owned();
        let asked = &frame["request"];

        if asked["subtype"] != "can_use_tool" {
            let subtype = asked["subtype"].as_str().unwrap_or_default();
            session
                .cli
                .refuse(&request, &format!("unsupported control request: {subtype}"));
            return Ok(());
        }

        let tool = asked["tool_name"].as_str().unwrap_or_default();
        let input = asked["input"].clone();
        let call = asked["tool_use_id"].as_str().unwrap_or(&request).to_owned();

        let (options, what) = match tool {
            "AskUserQuestion" => {
                let form = ask::form(&session.id, &call, &input).filter(|_| self.asks);
                let Some((form, questions)) = form else {
                    session.cli.respond(
                        &request,
                        ask::deny("There are no questions the user can answer here."),
                    );
                    return Ok(());
                };
                let reply = acp::bridge::ask_client(&self.cx, ClientRequest::Form(form));
                self.answers.push(
                    async move {
                        let asked = Asked::Questions { input, questions };
                        answer(request, asked, reply.await)
                    }
                    .boxed(),
                );
                return Ok(());
            }
            "ExitPlanMode" => (
                ask::plan_options(&session.before_plan),
                Asked::Plan {
                    input: input.clone(),
                },
            ),
            _ => {
                let suggested =
                    Some(asked["permission_suggestions"].clone()).filter(|rules| !rules.is_null());
                (
                    ask::tool_options(suggested.is_some()),
                    Asked::Tool {
                        input: input.clone(),
                        suggested,
                    },
                )
            }
        };
        let permission = RequestPermissionRequest::new(
            session.id.clone(),
            updates::asked(&call, tool, &input),
            options,
        );
        let reply = acp::bridge::ask_client(&self.cx, ClientRequest::Permission(permission));
        self.answers
            .push(async move { answer(request, what, reply.await) }.boxed());
        Ok(())
    }
}

/// What the CLI is told of the client's `reply` to what it `asked`.
fn answer(request: String, asked: Asked, reply: ClientReply) -> Answer {
    let (decision, mode) = match (asked, reply) {
        (Asked::Tool { input, suggested }, ClientReply::Permission(reply)) => {
            (ask::tool_decision(reply, input, suggested), None)
        }
        (Asked::Plan { input }, ClientReply::Permission(reply)) => ask::plan_decision(reply, input),
        (Asked::Questions { input, questions }, ClientReply::Form(reply)) => {
            (ask::answered(reply, input, &questions), None)
        }
        _ => (ask::deny("unexpected answer"), None),
    };

    Answer {
        request,
        decision,
        mode,
    }
}

/// The CLI answered a control request of ours with `answer`.
fn controlled(
    cx: &ConnectionTo<Client>,
    session: &mut Session,
    control: Control,
    answer: Result<&Value, String>,
) -> Result<()> {
    let refused = |error: String| agent_client_protocol::Error::internal_error().data(error);

    match control {
        Control::Initialize(start) => {
            let initialized = match answer {
                Ok(initialized) => initialized,
                Err(error) => {
                    start.refuse(refused(error.clone()))?;
                    anyhow::bail!("Claude Code did not initialize: {error}");
                }
            };
            session.models = models(initialized);
            session.model = session
                .models
                .iter()
                .map(|model| model.value.to_string())
                .find(|model| model == "default")
                .or_else(|| session.models.first().map(|model| model.value.to_string()))
                .unwrap_or_default();
            start.respond(
                cx,
                &session.id,
                modes(&session.mode),
                config_options(session),
                [],
            )?;

            let commands = commands(initialized);

            if !commands.is_empty() {
                notify(
                    cx,
                    &session.id,
                    SessionUpdate::AvailableCommandsUpdate(AvailableCommandsUpdate::new(commands)),
                )?;
            }
        }
        Control::Mode { mode, responder } => match answer {
            Ok(_) => {
                session.moved_to(&mode);
                responder.respond(SetSessionModeResponse::new())?;
            }
            Err(error) => responder.respond_with_error(refused(error))?,
        },
        Control::Model { model, responder } => match answer {
            Ok(_) => {
                session.model = model;
                responder.respond(SetSessionConfigOptionResponse::new(config_options(session)))?;
            }
            Err(error) => responder.respond_with_error(refused(error))?,
        },
        Control::Interrupt => {
            if let Err(error) = answer {
                log::warn!("Claude Code did not stop: {error}");
            }
        }
    }

    Ok(())
}

fn modes(current: &str) -> SessionModeState {
    SessionModeState::new(
        current.to_owned(),
        MODES
            .iter()
            .map(|(id, name)| SessionMode::new(*id, *name))
            .collect(),
    )
}

fn config_options(session: &Session) -> Vec<SessionConfigOption> {
    if session.models.is_empty() {
        return Vec::new();
    }

    vec![
        SessionConfigOption::select(
            MODEL,
            "Model",
            session.model.clone(),
            session.models.clone(),
        )
        .category(SessionConfigOptionCategory::Model),
    ]
}

/// The models an `initialize` answer offers.
fn models(initialized: &Value) -> Vec<SessionConfigSelectOption> {
    initialized["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|model| {
            let value = model["value"].as_str()?;
            let name = model["displayName"].as_str().unwrap_or(value);
            Some(
                SessionConfigSelectOption::new(value.to_owned(), name)
                    .description(model["description"].as_str().map(str::to_owned)),
            )
        })
        .take(acp::MAX_MODELS)
        .collect()
}

/// The slash commands an `initialize` answer offers.
fn commands(initialized: &Value) -> Vec<AvailableCommand> {
    initialized["commands"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|command| {
            let name = command["name"].as_str()?;
            let description = command["description"].as_str().unwrap_or_default();
            let hint = command["argumentHint"]
                .as_str()
                .filter(|hint| !hint.is_empty());
            Some(
                AvailableCommand::new(name, description).input(hint.map(|hint| {
                    AvailableCommandInput::Unstructured(UnstructuredCommandInput::new(hint))
                })),
            )
        })
        .collect()
}

/// The updates replaying session `id`'s transcript at `path`; none when it
/// cannot be read.
fn replayed(path: Option<PathBuf>, id: &str, in_wsl: bool) -> Vec<SessionUpdate> {
    let Some(path) = path else {
        return Vec::new();
    };

    updates::replay(&path, in_wsl).unwrap_or_else(|e| {
        log::warn!("the transcript of {id} was not replayed: {e}");
        Vec::new()
    })
}

/// The CLI's next frame, once it runs; `None` when it exited. A line too
/// long to read is skipped: nothing waits on one.
async fn next_frame(session: &mut Option<Session>) -> Option<Value> {
    let Some(session) = session else {
        return std::future::pending().await;
    };

    loop {
        if let Frame::Json(frame) = session.cli.pipe.frames.recv().await? {
            return Some(frame);
        }
    }
}

/// A prompt block as the CLI takes it.
fn content(block: &ContentBlock) -> Option<Value> {
    match block {
        ContentBlock::Text(text) => Some(json!({"type": "text", "text": text.text})),
        ContentBlock::Image(image) => Some(json!({
            "type": "image",
            "source": {"type": "base64", "media_type": image.mime_type, "data": image.data},
        })),
        ContentBlock::ResourceLink(link) => Some(json!({"type": "text", "text": link.uri})),
        _ => None,
    }
}

/// The MCP servers as `--mcp-config` takes them. In WSL a server on stdio
/// is a Windows program: it is named by its path inside the distribution,
/// and its variables cross over in `WSLENV`.
fn mcp_config(servers: &[McpServer], in_wsl: bool) -> Option<String> {
    let mut config = Map::new();

    for server in servers {
        let (name, entry) = match server {
            McpServer::Stdio(stdio) => (&stdio.name, acp::bridge::mcp_stdio(stdio, in_wsl)),
            McpServer::Http(http) => (
                &http.name,
                json!({
                    "type": "http",
                    "url": http.url,
                    "headers": acp::bridge::mcp_headers(&http.headers),
                }),
            ),
            McpServer::Sse(sse) => (
                &sse.name,
                json!({
                    "type": "sse",
                    "url": sse.url,
                    "headers": acp::bridge::mcp_headers(&sse.headers),
                }),
            ),
            _ => continue,
        };
        config.insert(name.clone(), entry);
    }

    (!config.is_empty()).then(|| json!({"mcpServers": config}).to_string())
}
