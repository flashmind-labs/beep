//! Shared ACP server plumbing for the node's in-process direct bridges
//! (`claude_direct` and `codex_direct`).

use crate::machine::wsl;
use agent_client_protocol::{
    Agent, Builder, Client, ConnectionTo, Dispatch, Handled, JsonRpcMessage, Responder,
    UntypedMessage, on_receive_dispatch,
    schema::ProtocolVersion,
    v1::schema::{
        AGENT_METHOD_NAMES, AgentCapabilities, CancelNotification, ContentBlock,
        CreateElicitationRequest, CreateElicitationResponse, HttpHeader, Implementation,
        InitializeRequest, InitializeResponse, LoadSessionRequest, LoadSessionResponse,
        McpCapabilities, McpServer, McpServerStdio, Meta, NewSessionRequest, NewSessionResponse,
        PromptCapabilities, RequestPermissionOutcome, RequestPermissionRequest,
        RequestPermissionResponse, ResumeSessionRequest, ResumeSessionResponse,
        SessionCapabilities, SessionConfigOption, SessionId, SessionModeState, SessionNotification,
        SessionResumeCapabilities, SessionSystemPromptCapabilities, SessionUpdate, SystemPrompt,
        SystemPromptMode,
    },
};
use anyhow::{Context, Result};
use futures::{FutureExt, future::BoxFuture};
use serde_json::{Map, Value, json};
use std::path::PathBuf;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::mpsc,
};

const EVENTS: usize = 64;
/// The name the node knows its Codex bridge by, which says it steers.
pub(in crate::agents) const CODEX_NAME: &str = "beep-node codex-acp";

/// What the client sent to an in-process ACP bridge.
pub(in crate::agents) enum Event {
    Request(UntypedMessage, Box<Responder>),
    Notification(UntypedMessage),
}

/// Serves ACP read from `input` and written to `output`, running `run` with
/// the client connection and incoming event stream.
pub(in crate::agents) async fn serve(
    name: &'static str,
    input: impl AsyncRead + Unpin + Send + 'static,
    output: impl AsyncWrite + Unpin + Send + 'static,
    run: impl AsyncFnOnce(ConnectionTo<Client>, mpsc::Receiver<Event>) -> Result<()>,
) -> Result<()> {
    let (events_tx, events) = mpsc::channel(EVENTS);
    let (oversized, _) = mpsc::channel(1);

    Builder::new(Agent)
        .name(name)
        .on_receive_dispatch(
            async move |dispatch: Dispatch, _cx| {
                let event = match dispatch {
                    Dispatch::Request(msg, responder) => Event::Request(msg, Box::new(responder)),
                    Dispatch::Notification(msg) => Event::Notification(msg),
                    message => {
                        return Ok(Handled::No {
                            message,
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
            super::transport(output, input, oversized),
            async move |cx: ConnectionTo<Client>| {
                run(cx, events).await.map_err(|e| {
                    log::error!("{e:#}");
                    agent_client_protocol::Error::internal_error().data(format!("{e:#}"))
                })
            },
        )
        .await
        .context("the ACP connection failed")
}

/// Parsed `session/new`, `session/load`, or `session/resume` request.
pub(in crate::agents) struct StartRequest {
    pub(in crate::agents) start: Start,
    pub(in crate::agents) session_id: Option<String>,
    pub(in crate::agents) cwd: PathBuf,
    pub(in crate::agents) mcp_servers: Vec<McpServer>,
    pub(in crate::agents) system_prompt: Option<SystemPrompt>,
    pub(in crate::agents) meta: Option<Meta>,
}

pub(in crate::agents) fn parse_start(
    message: &UntypedMessage,
    responder: Responder,
) -> Result<StartRequest> {
    let (method, params) = (message.method(), message.params());

    Ok(match method {
        "session/new" => {
            let r = NewSessionRequest::parse_message(method, params)?;
            StartRequest {
                start: Start::New(responder.cast()),
                session_id: None,
                cwd: r.cwd,
                mcp_servers: r.mcp_servers,
                system_prompt: r.system_prompt,
                meta: r.meta,
            }
        }
        "session/load" => {
            let r = LoadSessionRequest::parse_message(method, params)?;
            StartRequest {
                start: Start::Load(responder.cast(), Vec::new()),
                session_id: Some(r.session_id.0.to_string()),
                cwd: r.cwd,
                mcp_servers: r.mcp_servers,
                system_prompt: r.system_prompt,
                meta: r.meta,
            }
        }
        _ => {
            let r = ResumeSessionRequest::parse_message(method, params)?;
            StartRequest {
                start: Start::Resume(responder.cast()),
                session_id: Some(r.session_id.0.to_string()),
                cwd: r.cwd,
                mcp_servers: r.mcp_servers,
                system_prompt: r.system_prompt,
                meta: r.meta,
            }
        }
    })
}

/// How a session starts: new, or resumed with its history replayed or not.
pub(in crate::agents) enum Start {
    New(Responder<NewSessionResponse>),
    Load(Responder<LoadSessionResponse>, Vec<SessionUpdate>),
    Resume(Responder<ResumeSessionResponse>),
}

impl Start {
    pub(in crate::agents) fn is_new(&self) -> bool {
        matches!(self, Self::New(_))
    }

    pub(in crate::agents) fn is_load(&self) -> bool {
        matches!(self, Self::Load(..))
    }

    pub(in crate::agents) fn refuse(self, error: agent_client_protocol::Error) -> Result<()> {
        match self {
            Self::New(r) => r.respond_with_error(error)?,
            Self::Load(r, _) => r.respond_with_error(error)?,
            Self::Resume(r) => r.respond_with_error(error)?,
        }

        Ok(())
    }

    pub(in crate::agents) fn check(
        self,
        has_session: bool,
        system: Option<&SystemPrompt>,
    ) -> Result<Option<Self>> {
        if has_session {
            self.refuse(
                agent_client_protocol::Error::invalid_request()
                    .data("this process runs one session"),
            )?;
            return Ok(None);
        }

        if system.is_some_and(|s| s.mode != SystemPromptMode::Append) {
            self.refuse(
                agent_client_protocol::Error::invalid_params()
                    .data("only an appended system prompt is supported"),
            )?;
            return Ok(None);
        }

        Ok(Some(self))
    }

    pub(in crate::agents) fn respond(
        self,
        cx: &ConnectionTo<Client>,
        id: &SessionId,
        modes: SessionModeState,
        options: Vec<SessionConfigOption>,
        extra_replay: impl IntoIterator<Item = SessionUpdate>,
    ) -> Result<()> {
        match self {
            Self::New(r) => r.respond(
                NewSessionResponse::new(id.clone())
                    .modes(modes)
                    .config_options(options),
            )?,
            Self::Load(r, replay) => {
                for update in replay.into_iter().chain(extra_replay) {
                    notify(cx, id, update)?;
                }

                r.respond(
                    LoadSessionResponse::new()
                        .modes(modes)
                        .config_options(options),
                )?;
            }
            Self::Resume(r) => r.respond(
                ResumeSessionResponse::new()
                    .modes(modes)
                    .config_options(options),
            )?,
        }

        Ok(())
    }
}

pub(in crate::agents) enum ClientRequest {
    Permission(RequestPermissionRequest),
    Form(CreateElicitationRequest),
}

pub(in crate::agents) enum ClientReply {
    Permission(Result<RequestPermissionResponse, agent_client_protocol::Error>),
    Form(Result<CreateElicitationResponse, agent_client_protocol::Error>),
}

pub(in crate::agents) fn ask_client(
    cx: &ConnectionTo<Client>,
    request: ClientRequest,
) -> BoxFuture<'static, ClientReply> {
    match request {
        ClientRequest::Permission(r) => {
            let sent = cx.send_request(r).block_task();
            async move { ClientReply::Permission(sent.await) }.boxed()
        }
        ClientRequest::Form(form) => {
            let sent = cx.send_request(form).block_task();
            async move { ClientReply::Form(sent.await) }.boxed()
        }
    }
}

pub(in crate::agents) fn selected_option(
    reply: Result<RequestPermissionResponse, agent_client_protocol::Error>,
) -> Option<String> {
    match reply.ok()?.outcome {
        RequestPermissionOutcome::Selected(selected) => Some(selected.option_id.to_string()),
        _ => None,
    }
}

/// JSON Schema `const`/`title`/`description` option entries for question forms.
pub(in crate::agents) fn choice_options(options: &[Value]) -> Vec<Value> {
    options
        .iter()
        .filter_map(|option| {
            let label = option["label"].as_str()?;
            let mut choice = json!({"const": label, "title": label});
            if let Some(d) = option["description"].as_str()
                && !d.is_empty()
            {
                choice["description"] = json!(d);
            }
            Some(choice)
        })
        .collect()
}

pub(in crate::agents) fn accepts_forms(request: &InitializeRequest) -> bool {
    request
        .client_capabilities
        .elicitation
        .as_ref()
        .is_some_and(|e| e.form.is_some())
}

pub(in crate::agents) fn initialized(name: &str, sse: bool, steering: bool) -> InitializeResponse {
    let session = SessionCapabilities::new()
        .resume(SessionResumeCapabilities::new())
        .system_prompt(SessionSystemPromptCapabilities::new());
    let mcp = if sse {
        McpCapabilities::new().http(true).sse(true)
    } else {
        McpCapabilities::new().http(true)
    };
    let response = InitializeResponse::new(ProtocolVersion::V1)
        .agent_capabilities(
            AgentCapabilities::new()
                .load_session(true)
                .prompt_capabilities(PromptCapabilities::new().image(true).embedded_context(true))
                .mcp_capabilities(mcp)
                .session_capabilities(session),
        )
        .agent_info(Implementation::new(name, env!("CARGO_PKG_VERSION")));

    if !steering {
        return response;
    }

    let mut meta = Meta::new();
    meta.insert("steering".to_owned(), json!({"supported": true}));
    response.meta(meta)
}

pub(in crate::agents) fn notify(
    cx: &ConnectionTo<Client>,
    session: &SessionId,
    update: SessionUpdate,
) -> Result<()> {
    cx.send_notification(SessionNotification::new(session.clone(), update))?;
    Ok(())
}

pub(in crate::agents) fn unknown_session() -> agent_client_protocol::Error {
    agent_client_protocol::Error::invalid_params().data("no such session")
}

pub(in crate::agents) fn prompt_running() -> agent_client_protocol::Error {
    agent_client_protocol::Error::invalid_request().data("a prompt is running")
}

pub(in crate::agents) fn cancel_session_id(message: &UntypedMessage) -> Option<SessionId> {
    (message.method() == AGENT_METHOD_NAMES.session_cancel)
        .then(|| CancelNotification::parse_message(message.method(), message.params()).ok())
        .flatten()
        .map(|cancel| cancel.session_id)
}

pub(in crate::agents) fn system_text(prompt: Option<&SystemPrompt>) -> Option<String> {
    let text = prompt?
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    (!text.is_empty()).then_some(text)
}

pub(in crate::agents) fn mcp_stdio(stdio: &McpServerStdio, in_wsl: bool) -> Value {
    let mut env: Vec<(String, String)> = stdio
        .env
        .iter()
        .map(|var| (var.name.clone(), var.value.clone()))
        .collect();
    let command = match in_wsl.then(|| wsl::linux_path(&stdio.command)).flatten() {
        Some(command) => {
            let names: Vec<&str> = env.iter().map(|(name, _)| name.as_str()).collect();
            env.push(("WSLENV".to_owned(), names.join(":")));
            command.to_string()
        }
        None => stdio.command.display().to_string(),
    };
    let env: Map<String, Value> = env
        .into_iter()
        .map(|(name, value)| (name, Value::String(value)))
        .collect();

    json!({"command": command, "args": stdio.args, "env": env})
}

pub(in crate::agents) fn mcp_headers(headers: &[HttpHeader]) -> Map<String, Value> {
    headers
        .iter()
        .map(|h| (h.name.clone(), Value::String(h.value.clone())))
        .collect()
}
