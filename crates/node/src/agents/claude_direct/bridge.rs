//! The bridge's task: ACP from the client on one side, Claude Code's CLI on
//! the other. One session per process, as the node runs them.

use super::{
    ask,
    cli::{Cli, Launch},
    updates,
};
use crate::agents::acp;
use agent_client_protocol::{
    Agent, Builder, Client, ConnectionTo, Dispatch, Handled, JsonRpcMessage,
    Responder, UntypedMessage, on_receive_dispatch,
    schema::ProtocolVersion,
    v1::schema::{
        AgentCapabilities, AvailableCommand, AvailableCommandInput, AvailableCommandsUpdate,
        CancelNotification, ContentBlock, CurrentModeUpdate, Implementation, InitializeRequest,
        InitializeResponse, LoadSessionRequest, LoadSessionResponse, McpCapabilities, McpServer,
        NewSessionRequest, NewSessionResponse, PromptCapabilities, PromptRequest, PromptResponse,
        RequestPermissionRequest, ResumeSessionRequest, ResumeSessionResponse, SessionCapabilities,
        SessionConfigOption, SessionConfigOptionCategory, SessionConfigSelectOption, SessionId,
        SessionMode, SessionModeState, SessionNotification, SessionResumeCapabilities,
        SessionSystemPromptCapabilities, SessionUpdate, SetSessionConfigOptionRequest,
        SetSessionConfigOptionResponse, SetSessionModeRequest, SetSessionModeResponse,
        SystemPrompt, SystemPromptMode, UnstructuredCommandInput,
    },
};
use anyhow::{Context, Result};
use futures::{FutureExt, StreamExt, future::BoxFuture, stream::FuturesUnordered};
use serde_json::{Map, Value, json};
use crate::machine::wsl::{self, DistroName};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};
use tokio::{sync::mpsc, task::spawn_blocking};

/// How many messages from the client wait to be handled.
const EVENTS: usize = 64;

/// The session modes, as Claude Code names its permission modes.
const MODES: &[(&str, &str)] = &[
    ("default", "Default"),
    ("acceptEdits", "Accept Edits"),
    ("plan", "Plan Mode"),
    ("bypassPermissions", "Bypass Permissions"),
];

/// The config option that picks the model.
const MODEL: &str = "model";

/// Serves ACP on this process's stdio, driving Claude Code's CLI `program`
/// with the user's `args`, in the WSL distribution `wsl` when given, until
/// the client closes it or the CLI exits.
/// Logs go to stderr: stdout carries ACP.
pub async fn serve(program: String, args: Vec<String>, wsl: Option<&str>) -> Result<()> {
    let wsl = wsl.map(str::parse::<DistroName>).transpose()?;
    let (events_tx, mut events) = mpsc::channel(EVENTS);
    // Its reports go unread: a dropped message is logged where it is read.
    let (oversized, _) = mpsc::channel(1);

    Builder::new(Agent)
        .name("beep-node claude-acp")
        .on_receive_dispatch(
            async move |dispatch: Dispatch, _cx| {
                let event = match dispatch {
                    Dispatch::Request(message, responder) => {
                        Event::Request(message, Box::new(responder))
                    }
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
        .connect_with(
            acp::transport(tokio::io::stdout(), tokio::io::stdin(), oversized),
            async |cx: ConnectionTo<Client>| {
                let mut bridge = Bridge {
                    cx,
                    program,
                    args,
                    wsl,
                    asks: false,
                    session: None,
                    answers: FuturesUnordered::new(),
                };
                bridge.run(&mut events).await.map_err(|e| {
                    log::error!("{e:#}");
                    agent_client_protocol::Error::internal_error().data(format!("{e:#}"))
                })
            },
        )
        .await
        .context("the ACP connection failed")
}

/// What the client sent.
enum Event {
    Request(UntypedMessage, Box<Responder>),
    Notification(UntypedMessage),
}

/// How a session starts: new, or resumed with its history replayed
/// (`session/load`) or not (`session/resume`).
enum Start {
    New(Responder<NewSessionResponse>),
    /// With the updates that replay its transcript.
    Load(Responder<LoadSessionResponse>, Vec<SessionUpdate>),
    Resume(Responder<ResumeSessionResponse>),
}

impl Start {
    fn refuse(self, error: agent_client_protocol::Error) -> Result<()> {
        match self {
            Start::New(responder) => responder.respond_with_error(error)?,
            Start::Load(responder, _) => responder.respond_with_error(error)?,
            Start::Resume(responder) => responder.respond_with_error(error)?,
        }
        Ok(())
    }
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
    prompt: Option<Responder<PromptResponse>>,
    cancelled: bool,
    /// How much of the window the main agent's context fills.
    context: u64,
    /// The messages of this turn whose text came as stream deltas.
    streamed: HashSet<String>,
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
    program: String,
    args: Vec<String>,
    /// The WSL distribution the CLI runs in, on a Windows machine.
    wsl: Option<DistroName>,
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
        let method = message.method().to_owned();
        let params = message.params();

        match method.as_str() {
            "initialize" => {
                let request = InitializeRequest::parse_message(&method, params)?;
                self.asks = request
                    .client_capabilities
                    .elicitation
                    .as_ref()
                    .is_some_and(|elicitation| elicitation.form.is_some());
                responder.cast().respond(initialized())?;
            }
            "session/new" => {
                let request = NewSessionRequest::parse_message(&method, params)?;
                let id = uuid::Uuid::new_v4().to_string();
                let start = Start::New(responder.cast());
                self.start(&id, false, &request.cwd, &request.mcp_servers, request.system_prompt.as_ref(), start)?;
            }
            "session/load" => {
                let request = LoadSessionRequest::parse_message(&method, params)?;
                let id = request.session_id.0.to_string();
                let transcript = self.transcript(&request.cwd, &id).await;
                let replay = spawn_blocking(move || replayed(transcript, &id)).await?;
                let start = Start::Load(responder.cast(), replay);
                self.start(&request.session_id.0, true, &request.cwd, &request.mcp_servers, request.system_prompt.as_ref(), start)?;
            }
            "session/resume" => {
                let request = ResumeSessionRequest::parse_message(&method, params)?;
                let start = Start::Resume(responder.cast());
                self.start(&request.session_id.0, true, &request.cwd, &request.mcp_servers, request.system_prompt.as_ref(), start)?;
            }
            "session/prompt" => {
                let request = PromptRequest::parse_message(&method, params)?;
                let responder = responder.cast();
                let Some(session) = self.session.as_mut().filter(|session| session.id == request.session_id) else {
                    return Ok(responder.respond_with_error(unknown_session())?);
                };

                if session.prompt.is_some() {
                    return Ok(responder.respond_with_error(
                        agent_client_protocol::Error::invalid_request().data("a prompt is running"),
                    )?);
                }

                session.cli.user(request.prompt.iter().filter_map(content).collect());
                session.prompt = Some(responder);
                session.cancelled = false;
            }
            "session/set_mode" => {
                let request = SetSessionModeRequest::parse_message(&method, params)?;
                let responder = responder.cast();
                let mode = request.mode_id.to_string();
                let Some(session) = self.session.as_mut().filter(|session| session.id == request.session_id) else {
                    return Ok(responder.respond_with_error(unknown_session())?);
                };

                if !MODES.iter().any(|(id, _)| *id == mode) {
                    return Ok(responder.respond_with_error(
                        agent_client_protocol::Error::invalid_params().data(format!("no mode {mode}")),
                    )?);
                }

                let id = session.cli.control(json!({"subtype": "set_permission_mode", "mode": mode}));
                session.controls.insert(id, Control::Mode { mode, responder });
            }
            "session/set_config_option" => {
                let request = SetSessionConfigOptionRequest::parse_message(&method, params)?;
                let responder = responder.cast();
                let Some(session) = self.session.as_mut().filter(|session| session.id == request.session_id) else {
                    return Ok(responder.respond_with_error(unknown_session())?);
                };
                let model = request
                    .value
                    .as_value_id()
                    .map(|id| id.0.to_string())
                    .filter(|_| request.config_id.0.as_ref() == MODEL);
                let Some(model) = model else {
                    return Ok(responder.respond_with_error(
                        agent_client_protocol::Error::invalid_params().data("only the model can be set"),
                    )?);
                };

                let id = session.cli.control(json!({"subtype": "set_model", "model": model}));
                session.controls.insert(id, Control::Model { model, responder });
            }
            _ => responder.respond_with_error(agent_client_protocol::Error::method_not_found())?,
        }

        Ok(())
    }

    fn notification(&mut self, message: &UntypedMessage) {
        if message.method() != "session/cancel" {
            return;
        }

        let Ok(cancel) = CancelNotification::parse_message(message.method(), message.params()) else {
            return;
        };
        let Some(session) = self.session.as_mut().filter(|session| session.id == cancel.session_id) else {
            return;
        };

        if session.prompt.is_some() && !session.cancelled {
            session.cancelled = true;
            let id = session.cli.control(json!({"subtype": "interrupt"}));
            session.controls.insert(id, Control::Interrupt);
        }
    }

    /// Where Claude Code keeps session `id` it ran in `cwd`: under the
    /// user's home, or in WSL the distribution's.
    async fn transcript(&self, cwd: &Path, id: &str) -> Option<PathBuf> {
        let Some(distro) = &self.wsl else {
            return updates::transcript(cwd, id);
        };
        let cwd = wsl::linux_path(&cwd.to_string_lossy())?;
        let home = wsl::home(distro)
            .await
            .inspect_err(|e| log::warn!("the home folder in WSL {distro} is not known: {e:#}"))
            .ok()?;
        let folder = crate::agents::claude::slug(Path::new(&cwd));

        Some(wsl::windows_path(distro, &format!("{home}/.claude/projects/{folder}/{id}.jsonl")).into())
    }

    /// Starts the CLI on session `id` in `cwd`, resumed or new; `start` is
    /// answered once it initialized.
    fn start(
        &mut self,
        id: &str,
        resume: bool,
        cwd: &std::path::Path,
        mcp: &[McpServer],
        system: Option<&SystemPrompt>,
        start: Start,
    ) -> Result<()> {
        if self.session.is_some() {
            let refused = agent_client_protocol::Error::invalid_request().data("this process runs one session");
            return start.refuse(refused);
        }

        if system.is_some_and(|system| system.mode != SystemPromptMode::Append) {
            let refused = agent_client_protocol::Error::invalid_params()
                .data("only an appended system prompt is supported");
            return start.refuse(refused);
        }

        let launch = Launch {
            program: &self.program,
            extra: &self.args,
            cwd,
            session: id,
            resume,
            wsl: self.wsl.as_ref(),
            mcp: mcp_config(mcp, self.wsl.is_some()),
            system: system.map(system_text).filter(|text| !text.is_empty()),
            asks: self.asks,
        };
        let mut cli = Cli::start(&launch)?;
        let request = cli.control(json!({"subtype": "initialize"}));
        let session = self.session.insert(Session {
            id: SessionId::new(id.to_owned()),
            cli,
            mode: "default".to_owned(),
            before_plan: "default".to_owned(),
            models: Vec::new(),
            model: String::new(),
            controls: HashMap::new(),
            prompt: None,
            cancelled: false,
            context: 0,
            streamed: HashSet::new(),
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
                        _ => Err(response["error"].as_str().unwrap_or("Claude Code refused").to_owned()),
                    };
                    controlled(cx, session, control, answer)?;
                }
            }
            Some("control_request") => self.can_use_tool(&frame)?,
            Some("stream_event") => {
                if let Some(id) = frame["event"]["message"]["id"].as_str() {
                    session.streamed.insert(id.to_owned());
                }

                if let Some(update) = updates::delta(&frame) {
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
                    notify(cx, &session.id, SessionUpdate::CurrentModeUpdate(CurrentModeUpdate::new(mode.to_owned())))?;
                }
            }
            Some("result") if session.prompt.is_some() && updates::ends_prompt(&frame) => {
                if let Some(usage) = updates::usage_update(&frame, session.context) {
                    notify(cx, &session.id, SessionUpdate::UsageUpdate(usage))?;
                }

                session.streamed.clear();
                let cancelled = std::mem::take(&mut session.cancelled);
                if let Some(responder) = session.prompt.take() {
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
            session.cli.refuse(&request, &format!("unsupported control request: {subtype}"));
            return Ok(());
        }

        let tool = asked["tool_name"].as_str().unwrap_or_default();
        let input = asked["input"].clone();
        let call = asked["tool_use_id"].as_str().unwrap_or(&request).to_owned();

        let (options, what) = match tool {
            "AskUserQuestion" => {
                let form = ask::form(&session.id, &call, &input).filter(|_| self.asks);
                let Some((form, questions)) = form else {
                    session.cli.respond(&request, ask::deny("There are no questions the user can answer here."));
                    return Ok(());
                };
                let sent = self.cx.send_request(form).block_task();
                self.answers.push(
                    async move {
                        let asked = Asked::Questions { input, questions };
                        answer(request, asked, Reply::Form(sent.await))
                    }
                    .boxed(),
                );
                return Ok(());
            }
            "ExitPlanMode" => (ask::plan_options(&session.before_plan), Asked::Plan { input: input.clone() }),
            _ => {
                let suggested = Some(asked["permission_suggestions"].clone()).filter(|rules| !rules.is_null());
                (ask::tool_options(suggested.is_some()), Asked::Tool { input: input.clone(), suggested })
            }
        };
        let permission = RequestPermissionRequest::new(
            session.id.clone(),
            updates::asked(&call, tool, &input),
            options,
        );
        let sent = self.cx.send_request(permission).block_task();
        self.answers.push(async move { answer(request, what, Reply::Permission(sent.await)) }.boxed());
        Ok(())
    }
}

/// The client's answer to a request of ours.
enum Reply {
    Permission(Result<agent_client_protocol::v1::schema::RequestPermissionResponse, agent_client_protocol::Error>),
    Form(Result<agent_client_protocol::v1::schema::CreateElicitationResponse, agent_client_protocol::Error>),
}

/// What the CLI is told of the client's `reply` to what it `asked`.
fn answer(request: String, asked: Asked, reply: Reply) -> Answer {
    let (decision, mode) = match (asked, reply) {
        (Asked::Tool { input, suggested }, Reply::Permission(reply)) => {
            (ask::tool_decision(reply, input, suggested), None)
        }
        (Asked::Plan { input }, Reply::Permission(reply)) => ask::plan_decision(reply, input),
        (Asked::Questions { input, questions }, Reply::Form(reply)) => {
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
            let modes = modes(&session.mode);
            let options = config_options(session);

            match start {
                Start::New(responder) => responder.respond(
                    NewSessionResponse::new(session.id.clone())
                        .modes(modes)
                        .config_options(options),
                )?,
                Start::Load(responder, replay) => {
                    for update in replay {
                        notify(cx, &session.id, update)?;
                    }
                    responder.respond(LoadSessionResponse::new().modes(modes).config_options(options))?;
                }
                Start::Resume(responder) => responder.respond(
                    ResumeSessionResponse::new().modes(modes).config_options(options),
                )?,
            }

            let commands = commands(initialized);
            if !commands.is_empty() {
                notify(cx, &session.id, SessionUpdate::AvailableCommandsUpdate(AvailableCommandsUpdate::new(commands)))?;
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

/// What this agent can do. It replays a loaded session's transcript.
fn initialized() -> InitializeResponse {
    let session = SessionCapabilities::new()
        .resume(SessionResumeCapabilities::new())
        .system_prompt(SessionSystemPromptCapabilities::new());
    InitializeResponse::new(ProtocolVersion::V1)
        .agent_capabilities(
            AgentCapabilities::new()
                .load_session(true)
                .prompt_capabilities(PromptCapabilities::new().image(true).embedded_context(true))
                .mcp_capabilities(McpCapabilities::new().http(true).sse(true))
                .session_capabilities(session),
        )
        .agent_info(Implementation::new("beep-node claude-acp", env!("CARGO_PKG_VERSION")))
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
        SessionConfigOption::select(MODEL, "Model", session.model.clone(), session.models.clone())
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
            let hint = command["argumentHint"].as_str().filter(|hint| !hint.is_empty());
            Some(AvailableCommand::new(name, description).input(
                hint.map(|hint| AvailableCommandInput::Unstructured(UnstructuredCommandInput::new(hint))),
            ))
        })
        .collect()
}

/// The updates replaying session `id`'s transcript at `path`; none when it
/// cannot be read.
fn replayed(path: Option<PathBuf>, id: &str) -> Vec<SessionUpdate> {
    let Some(path) = path else {
        return Vec::new();
    };
    updates::replay(&path).unwrap_or_else(|e| {
        log::warn!("the transcript of {id} was not replayed: {e}");
        Vec::new()
    })
}

/// The CLI's next frame, once it runs; `None` when it exited.
async fn next_frame(session: &mut Option<Session>) -> Option<Value> {
    match session {
        Some(session) => session.cli.frames.recv().await,
        None => std::future::pending().await,
    }
}

fn notify(cx: &ConnectionTo<Client>, session: &SessionId, update: SessionUpdate) -> Result<()> {
    cx.send_notification(SessionNotification::new(session.clone(), update))?;
    Ok(())
}

fn unknown_session() -> agent_client_protocol::Error {
    agent_client_protocol::Error::invalid_params().data("no such session")
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

/// The text of a system prompt's blocks.
fn system_text(prompt: &SystemPrompt) -> String {
    prompt
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// The MCP servers as `--mcp-config` takes them. In WSL a server on stdio
/// is a Windows program: it is named by its path inside the distribution,
/// and its variables cross over in `WSLENV`.
fn mcp_config(servers: &[McpServer], in_wsl: bool) -> Option<String> {
    let pairs = |pairs: Vec<(String, String)>| -> Map<String, Value> {
        pairs.into_iter().map(|(name, value)| (name, Value::String(value))).collect()
    };
    let mut config = Map::new();

    for server in servers {
        let (name, entry) = match server {
            McpServer::Stdio(stdio) => {
                let mut env: Vec<(String, String)> =
                    stdio.env.iter().map(|var| (var.name.clone(), var.value.clone())).collect();
                let command = stdio.command.display().to_string();
                let command = match in_wsl {
                    true => {
                        let names: Vec<&str> = env.iter().map(|(name, _)| name.as_str()).collect();
                        env.push(("WSLENV".to_owned(), names.join(":")));
                        wsl::linux_path(&command).unwrap_or(command)
                    }
                    false => command,
                };
                (&stdio.name, json!({"command": command, "args": stdio.args, "env": pairs(env)}))
            }
            McpServer::Http(http) => (
                &http.name,
                json!({
                    "type": "http",
                    "url": http.url,
                    "headers": pairs(http.headers.iter().map(|h| (h.name.clone(), h.value.clone())).collect()),
                }),
            ),
            McpServer::Sse(sse) => (
                &sse.name,
                json!({
                    "type": "sse",
                    "url": sse.url,
                    "headers": pairs(sse.headers.iter().map(|h| (h.name.clone(), h.value.clone())).collect()),
                }),
            ),
            _ => continue,
        };
        config.insert(name.clone(), entry);
    }

    (!config.is_empty()).then(|| json!({"mcpServers": config}).to_string())
}

