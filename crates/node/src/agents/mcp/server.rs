//! MCP over one connection: newline-delimited JSON-RPC, as the stdio
//! transport speaks it. Calls run side by side, a few at a time, so a long
//! `remote_run` does not hold up the next call.

use super::{
    catalog::{self, ASK_USER, DEVICE, PROJECTS, READ_TRANSCRIPT, REMOTE_RUN, TEAM},
    media::Media,
};
use crate::{
    agents::{Answered, Asking, Line, LineReader},
    process::spawn_env,
};
use anyhow::Result;
use base64::{Engine, engine::general_purpose::STANDARD};
use beep_proto::SessionId;
use beep_proto::node::{
    agent::{AnswerOption, OptionKind},
    device::Frame,
    tools::{Approved, Reply, Request, Tool},
};
use futures::{StreamExt, stream::FuturesUnordered};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{future::Future, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncBufRead, AsyncWrite, AsyncWriteExt},
    time::timeout,
};

/// The longest message read; a longer one is dropped.
const MAX_MESSAGE: usize = 1024 * 1024;
/// The most calls of one connection running at once; the next waits.
const MAX_RUNNING: usize = 8;
/// How long a question waits for the user.
pub(super) const ASK_LIMIT: Duration = Duration::from_secs(60 * 60);
/// The MCP versions this server speaks, newest first.
const VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

/// Where calls go.
pub(crate) trait Backend: Sync {
    /// Runs one call on the node.
    fn call(&self, request: &Request) -> impl Future<Output = Result<Reply>> + Send;
    /// Reads a transcript independently of the acting tools/1 requests.
    fn read_transcript(&self, _args: Value) -> impl Future<Output = Result<Reply>> + Send {
        async {
            Ok(crate::agents::reply::fail(
                "Session transcripts are unavailable on this backend.",
            ))
        }
    }
    /// Runs `session`'s `device` call: emulators and simulators, which this
    /// node reaches over `device/1`.
    fn device(&self, _session: &SessionId, _args: Value) -> impl Future<Output = Called> + Send {
        async { Called::failed("Devices are unavailable on this backend.") }
    }
    /// Puts `question` to the user in `session` and waits for the answer.
    fn ask(
        &self,
        session: &SessionId,
        question: Asking,
    ) -> impl Future<Output = Result<Answered>> + Send;
    /// Whether `session` is a temporary reviewer, which is offered
    /// `submit_review`.
    fn reviewer(&self, session: &SessionId) -> bool;
    /// Whether `session` is the home assistant, which hears its brief and is
    /// offered `projects`.
    fn assistant(&self, session: &SessionId) -> bool;
    /// Whether the mesh has no other machine to run on, so the tools that
    /// reach one are left out.
    fn alone(&self) -> bool {
        false
    }
    /// Whether `session` works in a team project, which gets the team tools.
    fn team(&self, _session: &SessionId) -> bool {
        false
    }
    /// The media tools `session` gets from the saved API keys.
    fn media(&self, _session: &SessionId) -> Option<Media> {
        None
    }
}

/// Serves MCP for `session` until the reader ends and every call is
/// answered.
pub(super) async fn serve<B, R, W>(
    backend: &B,
    session: &SessionId,
    reader: R,
    mut writer: W,
) -> Result<()>
where
    B: Backend,
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut media = backend.media(session).map(Arc::new);
    // A key added or removed changes the media tools; the agent hears so.
    let mut keys = spawn_env::changes();
    let mut lines = LineReader::new(reader, MAX_MESSAGE);
    let mut running = FuturesUnordered::new();
    let mut open = true;

    loop {
        tokio::select! {
            line = lines.next(), if open && running.len() < MAX_RUNNING => match line? {
                None => open = false,
                Some(Line::TooLong(length)) => {
                    log::warn!("dropped an MCP message of {length} bytes");
                }
                Some(Line::Text(text)) if text.trim().is_empty() => {}
                Some(Line::Text(text)) => match serde_json::from_str::<Value>(&text) {
                    Ok(message) => running.push(answer(backend, session, media.clone(), message)),
                    Err(e) => {
                        let error = failure(Value::Null, -32700, &format!("not JSON: {e}"));
                        write(&mut writer, &error).await?;
                    }
                },
            },
            Ok(()) = keys.changed(), if open => {
                let before = listed(media.as_deref());
                media = backend.media(session).map(Arc::new);

                if listed(media.as_deref()) != before {
                    let note = json!({ "jsonrpc": "2.0", "method": "notifications/tools/list_changed" });
                    write(&mut writer, &note).await?;
                }
            }
            Some(answered) = running.next() => {
                if let Some(answered) = answered {
                    write(&mut writer, &answered).await?;
                }
            }
            else => return Ok(()),
        }
    }
}

async fn write<W: AsyncWrite + Unpin>(writer: &mut W, message: &Value) -> Result<()> {
    let mut line = serde_json::to_vec(message)?;
    line.push(b'\n');
    writer.write_all(&line).await?;
    writer.flush().await?;
    Ok(())
}

fn success(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn failure(id: Value, code: i32, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// The answer to one message; `None` for a notification or a response.
async fn answer<B: Backend>(
    backend: &B,
    session: &SessionId,
    media: Option<Arc<Media>>,
    message: Value,
) -> Option<Value> {
    let method = message.get("method")?.as_str()?.to_owned();
    let id = message.get("id")?.clone();
    let params = message.get("params").cloned().unwrap_or(Value::Null);

    Some(match method.as_str() {
        "initialize" => {
            let version = params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .filter(|asked| VERSIONS.contains(asked))
                .unwrap_or(VERSIONS[0]);
            let mut result = json!({
                "protocolVersion": version,
                "capabilities": { "tools": { "listChanged": true } },
                "serverInfo": { "name": super::SERVER_NAME, "version": env!("CARGO_PKG_VERSION") },
            });
            if let Some(brief) = media.as_deref().and_then(Media::brief) {
                result["instructions"] = json!(brief);
            }
            success(id, result)
        }
        "ping" => success(id, json!({})),
        "tools/list" => {
            let mut tools = catalog::tools(
                backend.reviewer(session),
                backend.alone(),
                backend.assistant(session),
            );

            if let Some(listed) = tools.as_array_mut() {
                if backend.team(session) {
                    listed.extend(catalog::team());
                }
                if let Some(media) = media.as_deref() {
                    listed.extend(media.listed());
                }
            }

            success(id, json!({ "tools": tools }))
        }
        "tools/call" => {
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let called = match media.as_deref().filter(|media| media.offers(name)) {
                Some(media) => {
                    let (text, failed) = media.call(name, args).await;
                    Called {
                        text,
                        failed,
                        image: None,
                    }
                }
                None => call(backend, session, name, args).await,
            };
            let mut content = vec![json!({ "type": "text", "text": called.text })];

            if let Some(frame) = &called.image {
                content.push(json!({
                    "type": "image",
                    "data": STANDARD.encode(&frame.jpeg),
                    "mimeType": "image/jpeg",
                }));
            }

            success(id, json!({ "content": content, "isError": called.failed }))
        }
        other => failure(id, -32601, &format!("unknown method {other}")),
    })
}

/// The media tools `media` lists, to see whether they changed.
fn listed(media: Option<&Media>) -> Vec<Value> {
    media.map(Media::listed).unwrap_or_default()
}

/// A tool's result for the agent, with a device's screen when it shows one.
#[derive(Debug, PartialEq)]
pub(crate) struct Called {
    pub(super) text: String,
    pub(super) failed: bool,
    pub(super) image: Option<Frame>,
}

impl Called {
    pub(super) fn ok(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            failed: false,
            image: None,
        }
    }

    pub(super) fn failed(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            failed: true,
            image: None,
        }
    }
}

impl From<Reply> for Called {
    fn from(reply: Reply) -> Self {
        match reply {
            Reply::Done { text, failed } => Self {
                text,
                failed,
                image: None,
            },
            other => Self::failed(format!("Unexpected reply {other:?}.")),
        }
    }
}

enum Route {
    AskUser(Value),
    ReadTranscript(Value),
    Device(Value),
    Node(Tool, Value),
}

async fn call<B: Backend>(backend: &B, session: &SessionId, name: &str, args: Value) -> Called {
    // Only the home assistant creates and imports projects and has threads,
    // and only a reviewer hands a review in.
    let hidden = ((name.starts_with(PROJECTS) || catalog::is_thread(name))
        && !backend.assistant(session))
        || (name == "submit_review" && !backend.reviewer(session));

    if hidden {
        return Called::failed(format!("There is no tool {name}."));
    }
    // Only sessions in a team project reach the team.
    if TEAM.contains(&name) && !backend.team(session) {
        return Called::failed(format!("There is no tool {name}."));
    }

    if let Err(error) = catalog::validate(name, &args) {
        return Called::failed(error);
    }

    let name = catalog::wire(name);
    let Some((operation, routed)) = route(name, args) else {
        return Called::failed(format!("There is no tool {name}."));
    };
    let mut trace = beep_proto::logging::RequestTrace::new(operation);
    let result = dispatch(backend, session, routed).await;
    trace.finish(!result.failed);

    result
}

fn route(name: &str, args: Value) -> Option<(&str, Route)> {
    if let Some(action) = name.strip_prefix(DEVICE) {
        return Some((name, Route::Device(with_action(args, action))));
    }

    // The desktop takes the project tools as one, by their action.
    if let Some(action) = name.strip_prefix(PROJECTS) {
        return Some((name, Route::Node(Tool::Projects, with_action(args, action))));
    }

    match name {
        ASK_USER => Some((name, Route::AskUser(args))),
        READ_TRANSCRIPT => Some((name, Route::ReadTranscript(args))),
        // `remote_run` goes as `run_on`, its name in `tools/1`.
        REMOTE_RUN => Some((name, Route::Node(Tool::RunOn, args))),
        _ => {
            let tool = name.parse::<Tool>().ok()?;
            Some((<&'static str>::from(tool), Route::Node(tool, args)))
        }
    }
}

async fn dispatch<B: Backend>(backend: &B, session: &SessionId, route: Route) -> Called {
    match route {
        Route::AskUser(args) => ask_user(backend, session, args).await,
        Route::ReadTranscript(args) => backend.read_transcript(args).await.map_or_else(
            |e| Called::failed(format!("Could not read the transcript: {e:#}")),
            Called::from,
        ),
        Route::Device(args) => backend.device(session, args).await,
        Route::Node(tool, args) => {
            let request = Request {
                session: session.clone(),
                tool,
                args: args.to_string(),
                approved: Approved::No,
            };
            backend
                .call(&request)
                .await
                .map_or_else(|e| Called::failed(format!("{e:#}")), Called::from)
        }
    }
}

fn with_action(mut args: Value, action: &str) -> Value {
    if let Some(map) = args.as_object_mut() {
        map.insert("action".to_owned(), Value::from(action));
    }

    args
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AskArgs {
    question: String,
    #[serde(default)]
    options: Vec<String>,
}

async fn ask_user<B: Backend>(backend: &B, session: &SessionId, args: Value) -> Called {
    let args: AskArgs = match serde_json::from_value(args) {
        Ok(args) => args,
        Err(e) => return Called::failed(format!("Bad arguments: {e}")),
    };

    if args.question.trim().is_empty() {
        return Called::failed("The question is empty.");
    }

    let options = args
        .options
        .iter()
        .take(8)
        .enumerate()
        .map(|(at, name)| AnswerOption {
            id: (at + 1).to_string(),
            name: name.clone(),
            kind: OptionKind::Other,
        })
        .collect();
    let question = Asking {
        text: args.question,
        detail: None,
        options,
    };
    let answered = timeout(ASK_LIMIT, backend.ask(session, question))
        .await
        .unwrap_or_else(|_| Err(anyhow::anyhow!("the user did not answer within an hour")));

    match answered {
        Ok(Answered::Picked(option)) => Called::ok(format!("The user picked: {}", option.name)),
        Ok(Answered::Said(text)) => Called::ok(format!("The user answered: {text}")),
        Err(e) => Called::failed(format!("No answer: {e:#}")),
    }
}

#[cfg(test)]
#[path = "../../tests/agents_mcp_server.rs"]
mod tests;
