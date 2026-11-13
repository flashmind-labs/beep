//! MCP over one connection: newline-delimited JSON-RPC, as the stdio
//! transport speaks it. Calls run side by side, a few at a time, so a long
//! `remote_run` does not hold up the next call.

use super::{
    catalog::{self, ASK_USER, DEVICE, READ_TRANSCRIPT, REMOTE_RUN},
    media::Media,
};
use crate::agents::{Answered, Asking, Line, LineReader};
use anyhow::Result;
use base64::{Engine, engine::general_purpose::STANDARD};
use beep_proto::node::{
    agent::{AnswerOption, OptionKind},
    device::Frame,
    tools::{Approved, Reply, Request, Tool},
};
use futures::{StreamExt, stream::FuturesUnordered};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{future::Future, time::Duration};
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
            Ok(super::reply::fail(
                "Session transcripts are unavailable on this backend.",
            ))
        }
    }
    /// Runs a `device` call: emulators and simulators, which this node
    /// reaches over `device/1`.
    fn device(&self, _args: Value) -> impl Future<Output = Called> + Send {
        async { Called::failed("Devices are unavailable on this backend.") }
    }
    /// Puts `question` to the user in `session` and waits for the answer.
    fn ask(&self, session: &str, question: Asking)
    -> impl Future<Output = Result<Answered>> + Send;
    /// Whether `session` is a temporary reviewer, which is offered
    /// `submit_review`.
    fn reviewer(&self, session: &str) -> bool;
    /// Whether the mesh has no other machine to run on, so the tools that
    /// reach one are left out.
    fn alone(&self) -> bool {
        false
    }
    /// The media tools `session` gets from the saved API keys.
    fn media(&self, _session: &str) -> Option<Media> {
        None
    }
}

/// Serves MCP for `session` until the reader ends and every call is
/// answered.
pub(super) async fn serve<B, R, W>(
    backend: &B,
    session: &str,
    reader: R,
    mut writer: W,
) -> Result<()>
where
    B: Backend,
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let media = backend.media(session);
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
                    Ok(message) => running.push(answer(backend, session, media.as_ref(), message)),
                    Err(e) => {
                        let error = failure(Value::Null, -32700, &format!("not JSON: {e}"));
                        write(&mut writer, &error).await?;
                    }
                },
            },
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
    session: &str,
    media: Option<&Media>,
    message: Value,
) -> Option<Value> {
    let method = message.get("method")?.as_str()?.to_owned();
    let id = message.get("id")?.clone();
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    Some(match method.as_str() {
        "initialize" => {
            let asked = params.get("protocolVersion").and_then(Value::as_str);
            let version = asked
                .filter(|asked| VERSIONS.contains(asked))
                .unwrap_or(VERSIONS[0]);
            let brief = media.and_then(Media::brief).unwrap_or_default();
            success(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": { "name": super::SERVER_NAME, "version": env!("CARGO_PKG_VERSION") },
                    "instructions": format!("{}{brief}", super::BRIEF),
                }),
            )
        }
        "ping" => success(id, json!({})),
        "tools/list" => {
            let mut tools = catalog::tools(backend.reviewer(session), backend.alone());

            if let (Some(listed), Some(media)) = (tools.as_array_mut(), media) {
                listed.extend(media.listed());
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
            let called = match media.filter(|media| media.offers(name)) {
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

async fn call<B: Backend>(backend: &B, session: &str, name: &str, args: Value) -> Called {
    if let Err(error) = catalog::validate(name, &args) {
        return Called::failed(error);
    }

    let (name, args) = match catalog::ungroup(name, args) {
        Ok(call) => call,
        Err(text) => return Called::failed(text),
    };
    let operation = match name {
        ASK_USER | READ_TRANSCRIPT | REMOTE_RUN | DEVICE => name,
        _ => name
            .parse::<Tool>()
            .map(<&'static str>::from)
            .unwrap_or("unknown tool"),
    };
    let mut trace = beep_proto::logging::RequestTrace::new(operation);
    let result = call_tool(backend, session, name, args).await;
    trace.finish(!result.failed);
    result
}

async fn call_tool<B: Backend>(backend: &B, session: &str, name: &str, args: Value) -> Called {
    match name {
        ASK_USER => return ask_user(backend, session, args).await,
        READ_TRANSCRIPT => {
            return match backend.read_transcript(args).await {
                Ok(reply) => reply.into(),
                Err(e) => Called::failed(format!("Could not read the transcript: {e:#}")),
            };
        }
        DEVICE => return backend.device(args).await,
        _ => {}
    }

    // `remote_run` goes as `run_on`, its name in `tools/1`, which is no MCP name.
    let parsed = match name {
        REMOTE_RUN => Ok(Tool::RunOn),
        "run_on" => Err(()),
        _ => name.parse::<Tool>().map_err(drop),
    };
    let tool = match parsed {
        // Coding agents get no memories, which are the assistant's, and read no machine logs.
        Ok(
            Tool::Unknown
            | Tool::MachineLog
            | Tool::StandingRules
            | Tool::AgentNote
            | Tool::Remember
            | Tool::Recall
            | Tool::UpdateMemory
            | Tool::Forget,
        )
        | Err(_) => {
            return Called::failed(format!("There is no tool {name}."));
        }
        // Only a reviewer hands a review in.
        Ok(Tool::SubmitReview) if !backend.reviewer(session) => {
            return Called::failed(format!("There is no tool {name}."));
        }
        Ok(tool) => tool,
    };
    let request = Request {
        session: session.to_owned(),
        tool,
        args: args.to_string(),
        approved: Approved::No,
    };

    match backend.call(&request).await {
        Ok(reply) => reply.into(),
        Err(e) => Called::failed(format!("{e:#}")),
    }
}

async fn asked<B: Backend>(backend: &B, session: &str, question: Asking) -> Result<Answered> {
    match timeout(ASK_LIMIT, backend.ask(session, question)).await {
        Ok(answered) => answered,
        Err(_) => anyhow::bail!("the user did not answer within an hour"),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AskArgs {
    question: String,
    #[serde(default)]
    options: Vec<String>,
}

async fn ask_user<B: Backend>(backend: &B, session: &str, args: Value) -> Called {
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

    match asked(backend, session, question).await {
        Ok(Answered::Picked(option)) => Called::ok(format!("The user picked: {}", option.name)),
        Ok(Answered::Said(text)) => Called::ok(format!("The user answered: {text}")),
        Err(e) => Called::failed(format!("No answer: {e:#}")),
    }
}

#[cfg(test)]
#[path = "../../tests/agents_mcp_server.rs"]
mod tests;
