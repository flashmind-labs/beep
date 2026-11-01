//! What the node needs around the official ACP crate: the requests it
//! sends, and the transport over the adapter's stdio, which caps each
//! message and skips lines that are not JSON-RPC.

use agent_client_protocol::{
    Lines,
    schema::{
        ProtocolVersion,
        v1::{
            ContentBlock, ImageContent, Implementation, InitializeRequest, InitializeResponse,
            PermissionOptionId, PromptRequest, RequestPermissionOutcome, RequestPermissionResponse,
            SelectedPermissionOutcome, SessionId, TextContent,
        },
    },
};
use base64::{Engine, engine::general_purpose::STANDARD};
use beep_proto::Upload;
use futures::{Sink, Stream, sink, stream};
use serde::Serialize;
use std::io;
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader},
    process::{ChildStdin, ChildStdout},
    sync::mpsc,
};

/// The longest message read from an agent; a longer one is dropped.
pub(super) const MAX_MESSAGE: usize = 16 * 1024 * 1024;

/// What `initialize` sends: this client reads and writes no files and runs
/// no terminals for the agent (the crate's defaults), so it does that work
/// itself.
pub(super) fn initialize() -> InitializeRequest {
    InitializeRequest::new(ProtocolVersion::V1)
        .client_info(Implementation::new("beep-node", env!("CARGO_PKG_VERSION")))
}

/// How an agent resumes a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ResumeMethod {
    /// `session/resume`, which replays nothing.
    Resume,
    /// `session/load`, which replays the history.
    Load,
}

/// How the agent the `initialize` answer describes resumes a session,
/// preferring `session/resume`; `None` when it does neither.
pub(super) fn resume_method(initialized: &InitializeResponse) -> Option<ResumeMethod> {
    let capabilities = &initialized.agent_capabilities;
    if capabilities.session_capabilities.resume.is_some() {
        Some(ResumeMethod::Resume)
    } else if capabilities.load_session {
        Some(ResumeMethod::Load)
    } else {
        None
    }
}

/// Claude Code's mode that never asks permission, as
/// `claude --dangerously-skip-permissions` runs.
pub(super) const BYPASS_MODE: &str = "bypassPermissions";

/// A message for the agent: its text, then any images.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct Prompt {
    pub(super) text: String,
    pub(super) images: Vec<Upload>,
}

impl From<String> for Prompt {
    fn from(text: String) -> Self {
        Self {
            text,
            images: Vec::new(),
        }
    }
}

impl Prompt {
    /// How the transcript shows it: the text, then a line per image, such
    /// as `[image: shot.png]`, which the apps draw as the picture.
    pub(super) fn shown(&self) -> String {
        let mut shown = self.text.clone();
        for image in &self.images {
            if !shown.is_empty() {
                shown.push('\n');
            }
            let name = if image.name.is_empty() {
                "image"
            } else {
                &image.name
            };
            shown.push_str(&format!("[image: {name}]"));
        }
        shown
    }
}

/// Whether the agent the `initialize` answer describes takes image blocks.
pub(super) fn takes_images(initialized: &InitializeResponse) -> bool {
    initialized.agent_capabilities.prompt_capabilities.image
}

/// `session/prompt` with the text first, then each image as an image block.
/// Only images go when there is no text.
pub(super) fn prompt(session: &SessionId, prompt: &Prompt) -> PromptRequest {
    let mut blocks = Vec::with_capacity(1 + prompt.images.len());
    if !prompt.text.is_empty() || prompt.images.is_empty() {
        blocks.push(ContentBlock::Text(TextContent::new(prompt.text.as_str())));
    }
    for image in &prompt.images {
        blocks.push(ContentBlock::Image(ImageContent::new(
            STANDARD.encode(&image.data),
            image.mime.as_str(),
        )));
    }
    PromptRequest::new(session.clone(), blocks)
}

pub(super) fn selected(option: &str) -> RequestPermissionResponse {
    RequestPermissionResponse::new(RequestPermissionOutcome::Selected(
        SelectedPermissionOutcome::new(PermissionOptionId::new(option)),
    ))
}

pub(super) fn cancelled() -> RequestPermissionResponse {
    RequestPermissionResponse::new(RequestPermissionOutcome::Cancelled)
}

/// The name an ACP enum has on the wire, such as `end_turn`, through its
/// own serde conversion; empty for a value that is not a string there.
pub(super) fn wire_name(value: &impl Serialize) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(name)) => name,
        _ => String::new(),
    }
}

/// The transport over an adapter's stdio. The crate writes from its own
/// actor, apart from reading, so a full stdin never stops stdout from
/// being read. Each oversized message dropped is reported on `oversized`
/// with its length.
pub(super) fn transport(
    stdin: ChildStdin,
    stdout: ChildStdout,
    oversized: mpsc::Sender<usize>,
) -> Lines<
    impl Sink<String, Error = io::Error> + Send + 'static,
    impl Stream<Item = io::Result<String>> + Send + 'static,
> {
    Lines::new(
        writer(stdin),
        messages(BufReader::new(stdout), MAX_MESSAGE, oversized),
    )
}

/// Writes each message the crate hands it as one line.
fn writer<W: AsyncWrite + Unpin + Send + 'static>(
    stdin: W,
) -> impl Sink<String, Error = io::Error> + Send + 'static {
    Box::pin(sink::unfold(stdin, async |mut stdin, message: String| {
        stdin.write_all(message.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await?;
        Ok::<_, io::Error>(stdin)
    }))
}

/// The JSON-RPC messages in `reader`, one per line, each at most `max`
/// bytes. Other lines (an npx warning, say) are logged and skipped rather
/// than answered with a parse error.
fn messages<R: AsyncBufRead + Unpin + Send + 'static>(
    reader: R,
    max: usize,
    oversized: mpsc::Sender<usize>,
) -> impl Stream<Item = io::Result<String>> + Send + 'static {
    let lines = LineReader::new(reader, max);
    stream::unfold(lines, move |mut lines| {
        let oversized = oversized.clone();
        async move {
            loop {
                match lines.next().await {
                    Ok(Some(Line::Text(text))) => {
                        let trimmed = text.trim_start();
                        if trimmed.starts_with('{') || trimmed.starts_with('[') {
                            return Some((Ok(text), lines));
                        }
                        if !trimmed.is_empty() {
                            log::debug!(
                                "agent wrote a line that is not JSON-RPC: {}",
                                super::cut(trimmed, 200)
                            );
                        }
                    }
                    Ok(Some(Line::TooLong(length))) => {
                        log::warn!("dropped a {length} byte message from the agent: over {max}");
                        // A full channel already carries a report.
                        let _ = oversized.try_send(length);
                    }
                    Ok(None) => return None,
                    Err(e) => return Some((Err(e), lines)),
                }
            }
        }
    })
}

/// One line read by a `LineReader`.
#[derive(Debug, PartialEq)]
pub(super) enum Line {
    /// The line without its newline.
    Text(String),
    /// A line longer than the cap, dropped; its length in bytes.
    TooLong(usize),
}

/// Reads lines without holding more than `max` bytes of any one: the rest
/// of a longer line is read and thrown away.
pub(super) struct LineReader<R> {
    reader: R,
    max: usize,
    line: Vec<u8>,
}

impl<R: AsyncBufRead + Unpin> LineReader<R> {
    pub(super) fn new(reader: R, max: usize) -> Self {
        Self {
            reader,
            max,
            line: Vec::new(),
        }
    }

    /// The next line; `None` at the end of the input.
    pub(super) async fn next(&mut self) -> io::Result<Option<Line>> {
        self.line.clear();
        let mut length = 0usize;
        loop {
            let available = self.reader.fill_buf().await?;
            if available.is_empty() {
                // The end of the input ends a last line that has no newline.
                return Ok((length > 0).then(|| self.finish(length)));
            }
            let (part, used, ended) = match available.iter().position(|&b| b == b'\n') {
                Some(at) => (&available[..at], at + 1, true),
                None => (available, available.len(), false),
            };
            length = length.saturating_add(part.len());
            if length <= self.max {
                self.line.extend_from_slice(part);
            } else {
                // Nothing of an oversized line is kept.
                self.line = Vec::new();
            }
            self.reader.consume(used);
            if ended {
                return Ok(Some(self.finish(length)));
            }
        }
    }

    fn finish(&mut self, length: usize) -> Line {
        if length > self.max {
            return Line::TooLong(length);
        }
        let mut text = String::from_utf8_lossy(&self.line).into_owned();
        if text.ends_with('\r') {
            text.pop();
        }
        Line::Text(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::v1::{
        AgentCapabilities, SessionCapabilities, SessionResumeCapabilities,
    };
    use futures::StreamExt;

    #[test]
    fn resume_prefers_session_resume_to_load() {
        let answer = |capabilities| {
            InitializeResponse::new(ProtocolVersion::V1).agent_capabilities(capabilities)
        };
        let both = AgentCapabilities::new()
            .load_session(true)
            .session_capabilities(
                SessionCapabilities::new().resume(SessionResumeCapabilities::new()),
            );
        assert_eq!(resume_method(&answer(both)), Some(ResumeMethod::Resume));
        let load = AgentCapabilities::new().load_session(true);
        assert_eq!(resume_method(&answer(load)), Some(ResumeMethod::Load));
        assert_eq!(resume_method(&answer(AgentCapabilities::new())), None);
    }

    #[tokio::test]
    async fn lines_over_the_cap_are_dropped_whole() {
        let input = format!("short\n{}\nafter\r\nlast", "x".repeat(100));
        // A small buffer, so the long line spans many reads.
        let reader = BufReader::with_capacity(8, input.as_bytes());
        let mut lines = LineReader::new(reader, 10);
        let mut read = Vec::new();
        while let Some(line) = lines.next().await.unwrap() {
            read.push(line);
        }
        assert_eq!(
            read,
            [
                Line::Text("short".into()),
                Line::TooLong(100),
                Line::Text("after".into()),
                Line::Text("last".into()),
            ]
        );
    }

    #[tokio::test]
    async fn an_oversized_message_is_reported_and_skipped() {
        let input = format!(
            "npm warn deprecated\n{{\"a\":\"{}\"}}\n{{\"b\":1}}\n",
            "x".repeat(64)
        );
        let (oversized, mut reports) = mpsc::channel(1);
        let read: Vec<String> = messages(std::io::Cursor::new(input.into_bytes()), 32, oversized)
            .map(|line| line.unwrap())
            .collect()
            .await;
        assert_eq!(read, [r#"{"b":1}"#]);
        assert_eq!(reports.recv().await, Some(72));
    }

    #[test]
    fn images_follow_the_text_as_image_blocks() {
        let shot = Upload {
            name: "shot.png".into(),
            mime: "image/png".into(),
            data: vec![0x89, b'P', b'N', b'G'],
        };
        let message = Prompt {
            text: "The button overlaps".into(),
            images: vec![shot.clone()],
        };
        let sent = serde_json::to_value(prompt(&SessionId::new("s"), &message)).unwrap();
        let blocks = sent["prompt"].as_array().unwrap();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0]["type"], "text");
        assert_eq!(blocks[0]["text"], "The button overlaps");
        assert_eq!(blocks[1]["type"], "image");
        assert_eq!(blocks[1]["mimeType"], "image/png");
        assert_eq!(blocks[1]["data"], STANDARD.encode(&shot.data));
        assert_eq!(message.shown(), "The button overlaps\n[image: shot.png]");

        let only = Prompt {
            text: String::new(),
            images: vec![shot],
        };
        let sent = serde_json::to_value(prompt(&SessionId::new("s"), &only)).unwrap();
        assert_eq!(sent["prompt"][0]["type"], "image", "no empty text block");
        assert_eq!(only.shown(), "[image: shot.png]");
    }

    #[test]
    fn enums_read_as_their_wire_names() {
        use agent_client_protocol::schema::v1::StopReason;
        assert_eq!(wire_name(&StopReason::EndTurn), "end_turn");
        assert_eq!(wire_name(&StopReason::MaxTurnRequests), "max_turn_requests");
    }
}
