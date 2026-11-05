//! OpenRouter's chat completions, streamed, and its error text.

use crate::{AssistantError, Call, Event, Setup, tools::MAX_CALLS_PER_ROUND};
use futures::StreamExt;
use secrecy::ExposeSecret;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::mpsc::Sender;

/// What one request produced, filled in as it streams so a cut-off or failed request still
/// leaves its text.
#[derive(Debug, Default)]
pub(crate) struct Reply {
    pub text: String,
    pub reasoning: String,
    /// Set once the stream ends cleanly.
    pub calls: Vec<Call>,
}

pub(crate) enum Finish {
    Complete,
    Cancelled,
}

#[derive(Deserialize)]
struct Chunk {
    #[serde(default)]
    choices: Vec<Choice>,
    error: Option<ErrorBody>,
}

#[derive(Deserialize)]
struct Choice {
    #[serde(default)]
    delta: Delta,
    finish_reason: Option<String>,
}

#[derive(Deserialize, Default)]
struct Delta {
    content: Option<String>,
    reasoning: Option<String>,
    tool_calls: Option<Vec<ToolDelta>>,
}

#[derive(Deserialize)]
struct ToolDelta {
    index: Option<usize>,
    id: Option<String>,
    function: Option<FunctionDelta>,
}

#[derive(Deserialize)]
struct FunctionDelta {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Deserialize)]
struct ErrorBody {
    message: Option<String>,
}

#[derive(Deserialize)]
struct ErrorEnvelope {
    error: Option<ErrorBody>,
}

/// The text of an error reply: OpenRouter's `error.message`, else the body as it came.
pub(crate) fn error_text(status: reqwest::StatusCode, body: &str) -> String {
    let message = serde_json::from_str::<ErrorEnvelope>(body)
        .ok()
        .and_then(|envelope| envelope.error)
        .and_then(|error| error.message)
        .filter(|message| !message.trim().is_empty());
    match message {
        Some(message) => message,
        None if body.trim().is_empty() => format!("The service answered {status}."),
        None => body.chars().take(500).collect(),
    }
}

/// Turns a reply with a failing status into its error.
pub(crate) async fn check(
    response: reqwest::Response,
) -> Result<reqwest::Response, AssistantError> {
    if response.status().is_success() {
        return Ok(response);
    }
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    Err(AssistantError::Api(error_text(status, &body)))
}

/// A tool call that arrives in pieces, by its index.
#[derive(Default)]
struct PartialCall {
    id: String,
    name: String,
    arguments: String,
}

/// The most a line of the stream may hold before it is taken for a broken reply.
const MAX_LINE_BYTES: usize = 4 << 20;

/// Asks `model` for the next message of `messages` and streams it into `reply`.
pub(crate) async fn stream_completion(
    setup: &Setup,
    model: &str,
    messages: Vec<Value>,
    tools: &[Value],
    reply: &mut Reply,
    events: &Sender<Event>,
) -> Result<Finish, AssistantError> {
    let (endpoint, cancel) = (&setup.openrouter, &setup.cancel);
    let mut body = json!({
        "model": model,
        "messages": messages,
        "stream": true,
        "include_reasoning": true,
    });
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools.to_vec());
    }
    let request = setup
        .client
        .post(format!("{}/chat/completions", endpoint.base))
        .bearer_auth(endpoint.key.expose_secret())
        .header("X-Title", "Beep")
        .json(&body)
        .send();
    let response = tokio::select! {
        biased;
        () = cancel.cancelled() => return Ok(Finish::Cancelled),
        sent = request => check(sent?).await?,
    };
    let mut stream = response.bytes_stream();
    let mut buffer: Vec<u8> = Vec::new();
    let mut partial: Vec<PartialCall> = Vec::new();
    // The model said it was done: a finish reason came, or `[DONE]`.
    let mut ended = false;
    loop {
        let next = tokio::select! {
            biased;
            () = cancel.cancelled() => return Ok(Finish::Cancelled),
            next = stream.next() => next,
        };
        let Some(bytes) = next else { break };
        buffer.extend_from_slice(&bytes?);
        while let Some(end) = buffer.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = buffer.drain(..=end).collect();
            let line = String::from_utf8_lossy(&line);
            let Some(data) = line.trim().strip_prefix("data:") else {
                // Blank separators and `: OPENROUTER PROCESSING` comments.
                continue;
            };
            let data = data.trim();
            if data == "[DONE]" {
                reply.calls = finished_calls(partial);
                return Ok(Finish::Complete);
            }
            let Ok(chunk) = serde_json::from_str::<Chunk>(data) else {
                continue;
            };
            if let Some(error) = chunk.error {
                let message = error.message.unwrap_or_else(|| "The model failed.".into());
                return Err(AssistantError::Api(message));
            }
            for choice in chunk.choices {
                ended |= choice.finish_reason.is_some();
                apply(choice.delta, reply, &mut partial, events).await?;
            }
        }
        if buffer.len() > MAX_LINE_BYTES {
            return Err(AssistantError::Api("The reply was not readable.".into()));
        }
    }
    if !ended {
        return Err(AssistantError::Api("The reply was cut off.".into()));
    }
    reply.calls = finished_calls(partial);
    Ok(Finish::Complete)
}

async fn apply(
    delta: Delta,
    reply: &mut Reply,
    partial: &mut Vec<PartialCall>,
    events: &Sender<Event>,
) -> Result<(), AssistantError> {
    if let Some(text) = delta.reasoning.filter(|text| !text.is_empty()) {
        reply.reasoning.push_str(&text);
        // A closed receiver means the window is gone; its owner cancels the run.
        events.send(Event::Reasoning(text)).await.ok();
    }
    if let Some(text) = delta.content.filter(|text| !text.is_empty()) {
        reply.text.push_str(&text);
        events.send(Event::Text(text)).await.ok();
    }
    for piece in delta.tool_calls.unwrap_or_default() {
        let index = piece.index.unwrap_or(0);
        if index >= MAX_CALLS_PER_ROUND * 2 {
            return Err(AssistantError::Api(
                "The model sent too many tool calls.".into(),
            ));
        }
        if partial.len() <= index {
            partial.resize_with(index + 1, PartialCall::default);
        }
        let call = &mut partial[index];
        if let Some(id) = piece.id {
            call.id = id;
        }
        if let Some(function) = piece.function {
            if let Some(name) = function.name {
                call.name.push_str(&name);
            }
            if let Some(arguments) = function.arguments {
                call.arguments.push_str(&arguments);
            }
        }
    }
    Ok(())
}

fn finished_calls(partial: Vec<PartialCall>) -> Vec<Call> {
    partial
        .into_iter()
        .enumerate()
        .filter(|(_, call)| !call.name.is_empty())
        .map(|(index, call)| Call {
            id: if call.id.is_empty() {
                format!("call_{index}")
            } else {
                call.id
            },
            name: call.name,
            arguments: call.arguments,
        })
        .collect()
}
