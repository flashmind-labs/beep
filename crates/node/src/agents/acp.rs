//! What the node needs around the official ACP crate: the requests it
//! sends, and the transport over the adapter's stdio, which caps each
//! message and skips lines that are not JSON-RPC.

use super::images::{Picture, image_line};
use agent_client_protocol::{
    Lines, UntypedMessage,
    schema::{
        ProtocolVersion,
        v1::{
            AGENT_METHOD_NAMES, ClientCapabilities, ClientSessionCapabilities,
            CompactionCapabilities, ContentBlock, ImageContent, Implementation, InitializeRequest,
            InitializeResponse, LoadSessionRequest, McpServer, NewSessionRequest,
            NewSessionResponse, PermissionOptionId, PromptRequest, RequestPermissionOutcome,
            RequestPermissionResponse, ResumeSessionRequest, SelectedPermissionOutcome,
            SessionConfigId, SessionConfigKind, SessionConfigOption, SessionConfigOptionCategory,
            SessionConfigSelectOption, SessionConfigSelectOptions, SessionId,
            SetSessionConfigOptionRequest, TextContent,
        },
    },
};
use base64::{Engine, engine::general_purpose::STANDARD};
use beep_proto::{
    ApiError, Upload, check_agent_images,
    node::agent::{AnswerOption, Effort, ModelOption, OptionKind},
};
use futures::{Sink, Stream, sink, stream};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{io, path::Path};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader},
    process::{ChildStdin, ChildStdout},
    sync::mpsc,
};

/// The longest message read from an agent; a longer one is dropped.
pub(super) const MAX_MESSAGE: usize = 16 * 1024 * 1024;

/// What `initialize` sends: this client reads and writes no files and runs
/// no terminals for the agent (the crate's defaults), so it does that work
/// itself. It takes `compaction_update`s, which the transcript shows.
pub(super) fn initialize() -> InitializeRequest {
    let session = ClientSessionCapabilities::new().compaction(CompactionCapabilities::new());
    InitializeRequest::new(ProtocolVersion::V1)
        .client_capabilities(ClientCapabilities::new().session(session))
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
/// preferring `session/load`, which replays the conversation so the
/// session shows its history; `None` when it does neither.
pub(super) fn resume_method(initialized: &InitializeResponse) -> Option<ResumeMethod> {
    let capabilities = &initialized.agent_capabilities;
    if capabilities.load_session {
        Some(ResumeMethod::Load)
    } else if capabilities.session_capabilities.resume.is_some() {
        Some(ResumeMethod::Resume)
    } else {
        None
    }
}

/// The most models kept from one agent's list.
pub(super) const MAX_MODELS: usize = 500;

/// The models an agent offers in its answer to `session/new`, and how it
/// switches between them.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct Offered {
    /// The model the session runs on, when it says.
    pub(super) current: Option<String>,
    pub(super) models: Vec<ModelOption>,
    pub(super) switch: Option<ModelSwitch>,
    /// Its config option of category `thought_level`, which sets how hard
    /// it thinks, when it has one.
    pub(super) thinking: Option<SessionConfigId>,
}

/// How an agent switches a session's model.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum ModelSwitch {
    /// `session/set_config_option` on its selector of category `model`.
    ConfigOption(SessionConfigId),
    /// `session/set_model`, from ACP's unstable session models, which the
    /// Claude Code, Codex and OpenCode adapters answer.
    SetModel,
}

/// `models` in an answer to `session/new`: ACP's unstable session models.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionModels {
    #[serde(default)]
    available_models: Vec<AcpModel>,
    #[serde(default)]
    current_model_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AcpModel {
    model_id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

/// `session/new` in `cwd`, sent untyped so the answer keeps `models`, which
/// the crate's typed answer drops.
pub(super) fn new_session(
    cwd: &Path,
    mcp_servers: Vec<McpServer>,
) -> Result<UntypedMessage, agent_client_protocol::Error> {
    UntypedMessage::new(
        AGENT_METHOD_NAMES.session_new,
        NewSessionRequest::new(cwd).mcp_servers(mcp_servers),
    )
}

/// `session/load` or `session/resume` of `session` in `cwd`, sent untyped
/// so the answer keeps `models`, as `new_session` does.
pub(super) fn resume_session(
    method: ResumeMethod,
    session: SessionId,
    cwd: &Path,
    mcp_servers: Vec<McpServer>,
) -> Result<UntypedMessage, agent_client_protocol::Error> {
    match method {
        ResumeMethod::Load => UntypedMessage::new(
            AGENT_METHOD_NAMES.session_load,
            LoadSessionRequest::new(session, cwd).mcp_servers(mcp_servers),
        ),
        ResumeMethod::Resume => UntypedMessage::new(
            AGENT_METHOD_NAMES.session_resume,
            ResumeSessionRequest::new(session, cwd).mcp_servers(mcp_servers),
        ),
    }
}

/// The session an answer to `session/new` made, and the models it offers.
pub(super) fn session_created(
    answer: Value,
) -> Result<(NewSessionResponse, Offered), serde_json::Error> {
    let offered = offers(&answer);
    Ok((serde_json::from_value(answer)?, offered))
}

/// The models and thinking option an answer to `session/new`,
/// `session/load` or `session/resume` offers.
pub(super) fn offers(answer: &Value) -> Offered {
    Offered {
        thinking: thinking_option(answer),
        ..offered_models(answer)
    }
}

/// The config options in an answer that offers them, skipping any that do
/// not read. The answer to `session/load` has no `sessionId`, so the
/// options are read on their own rather than as a `NewSessionResponse`.
fn config_options(answer: &Value) -> Vec<SessionConfigOption> {
    let Some(options) = answer.get("configOptions").and_then(Value::as_array) else {
        return Vec::new();
    };
    options
        .iter()
        .filter_map(|option| SessionConfigOption::deserialize(option).ok())
        .collect()
}

/// The selector of category `thought_level` in an answer:
/// Claude Code's `effort`, Codex's `reasoning_effort` and pi's
/// `thought_level`, which take the values `Effort` names.
fn thinking_option(answer: &Value) -> Option<SessionConfigId> {
    config_options(answer).into_iter().find_map(|option| {
        let thinking = matches!(option.kind, SessionConfigKind::Select(_))
            && matches!(
                option.category,
                Some(SessionConfigOptionCategory::ThoughtLevel)
            );
        thinking.then_some(option.id)
    })
}

/// The request that sets how hard `session` thinks, on its option `id`.
pub(super) fn set_effort(
    session: &SessionId,
    id: &SessionConfigId,
    effort: Effort,
) -> Result<UntypedMessage, agent_client_protocol::Error> {
    UntypedMessage::new(
        AGENT_METHOD_NAMES.session_set_config_option,
        SetSessionConfigOptionRequest::new(session.clone(), id.clone(), effort.as_str()),
    )
}

/// The models an answer offers: its config option of
/// category `model` when it has one, else its unstable `models`.
pub(super) fn offered_models(answer: &Value) -> Offered {
    let selector = config_options(answer).into_iter().find_map(|option| {
        let SessionConfigKind::Select(select) = option.kind else {
            return None;
        };
        matches!(option.category, Some(SessionConfigOptionCategory::Model))
            .then_some((option.id, select))
    });
    if let Some((id, select)) = selector {
        let choices: Vec<SessionConfigSelectOption> = match select.options {
            SessionConfigSelectOptions::Ungrouped(options) => options,
            SessionConfigSelectOptions::Grouped(groups) => {
                groups.into_iter().flat_map(|group| group.options).collect()
            }
            _ => Vec::new(),
        };
        return Offered {
            current: Some(select.current_value.to_string()),
            models: choices
                .into_iter()
                .take(MAX_MODELS)
                .map(|choice| ModelOption {
                    id: choice.value.to_string(),
                    name: choice.name,
                    description: choice.description,
                })
                .collect(),
            switch: Some(ModelSwitch::ConfigOption(id)),
            thinking: None,
        };
    }
    let Some(models) = answer
        .get("models")
        .and_then(|models| serde_json::from_value::<SessionModels>(models.clone()).ok())
    else {
        return Offered::default();
    };
    Offered {
        current: models.current_model_id,
        models: models
            .available_models
            .into_iter()
            .take(MAX_MODELS)
            .map(|model| ModelOption {
                name: model.name.unwrap_or_else(|| model.model_id.clone()),
                id: model.model_id,
                description: model.description,
            })
            .collect(),
        switch: Some(ModelSwitch::SetModel),
        thinking: None,
    }
}

/// The request that switches `session` to `model` the way `switch` says.
pub(super) fn switch_model(
    session: &SessionId,
    switch: &ModelSwitch,
    model: &str,
) -> Result<UntypedMessage, agent_client_protocol::Error> {
    match switch {
        ModelSwitch::ConfigOption(id) => UntypedMessage::new(
            AGENT_METHOD_NAMES.session_set_config_option,
            SetSessionConfigOptionRequest::new(session.clone(), id.clone(), model),
        ),
        ModelSwitch::SetModel => UntypedMessage::new(
            "session/set_model",
            serde_json::json!({ "sessionId": session, "modelId": model }),
        ),
    }
}

/// The option that allows what a permission request asks, preferring one
/// that keeps allowing it. `None` when nothing in it rejects: then it is a
/// choice put to the user, such as a pi extension's select, not a request
/// for permission.
///
/// Claude Code's plan approval leads with options that also clear the
/// context (`exit-plan-clear-*`); those are passed over.
pub(super) fn allow_option(options: &[AnswerOption]) -> Option<&AnswerOption> {
    if !options.iter().any(|option| option.kind.rejects()) {
        return None;
    }
    let keeps_context = |option: &&AnswerOption| !option.id.starts_with("exit-plan-clear");
    options
        .iter()
        .filter(keeps_context)
        .find(|option| option.kind == OptionKind::AllowAlways)
        .or_else(|| {
            options
                .iter()
                .filter(keeps_context)
                .find(|option| option.kind == OptionKind::AllowOnce)
        })
}

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
    /// How the transcript shows it: the text, then a line per image by its
    /// key, `[image: image-<key>]`, which the apps ask the node for.
    pub(super) fn shown(&self) -> String {
        let mut shown = self.text.clone();
        for image in &self.images {
            if !shown.is_empty() {
                shown.push('\n');
            }
            shown.push_str(&image_line(&image.data));
        }
        shown
    }

    /// Its images, for the transcript's lines to show.
    pub(super) fn pictures(&self) -> impl Iterator<Item = Picture> + '_ {
        self.images.iter().map(|image| Picture {
            mime: image.mime.clone(),
            data: image.data.clone(),
        })
    }

    /// A prompt with images, refused as the server refuses it: too many,
    /// too large, or not the image it says it is. The node checks again
    /// because a server is not the only thing that can reach it.
    pub(super) fn with_images(text: String, images: Vec<Upload>) -> Result<Self, String> {
        check_agent_images(&images).map_err(|e| match e {
            ApiError::BadRequest { message } => message,
            other => other.to_string(),
        })?;
        Ok(Self { text, images })
    }

    /// The bytes it holds, to bound what waits in a queue.
    pub(super) fn size(&self) -> usize {
        self.images.iter().fold(self.text.len(), |n, image| {
            n.saturating_add(image.data.len())
        })
    }
}

/// Whether the agent the `initialize` answer describes takes image blocks.
pub(super) fn takes_images(initialized: &InitializeResponse) -> bool {
    initialized.agent_capabilities.prompt_capabilities.image
}

/// `session/prompt` with `brief` (Beep's word to the agent) and the text
/// first, then each image as an image block. Only images go when there is
/// no text.
pub(super) fn prompt(session: &SessionId, prompt: &Prompt, brief: Option<&str>) -> PromptRequest {
    let mut blocks = Vec::with_capacity(2 + prompt.images.len());
    if let Some(brief) = brief {
        blocks.push(ContentBlock::Text(TextContent::new(brief)));
    }
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
pub(crate) enum Line {
    /// The line without its newline.
    Text(String),
    /// A line longer than the cap, dropped; its length in bytes.
    TooLong(usize),
}

/// Reads lines without holding more than `max` bytes of any one: the rest
/// of a longer line is read and thrown away.
pub(crate) struct LineReader<R> {
    reader: R,
    max: usize,
    line: Vec<u8>,
}

impl<R: AsyncBufRead + Unpin> LineReader<R> {
    pub(crate) fn new(reader: R, max: usize) -> Self {
        Self {
            reader,
            max,
            line: Vec::new(),
        }
    }

    /// The next line; `None` at the end of the input.
    pub(crate) async fn next(&mut self) -> io::Result<Option<Line>> {
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
    use beep_proto::image_key;
    use futures::StreamExt;

    #[test]
    fn resume_prefers_load_to_session_resume() {
        let answer = |capabilities| {
            InitializeResponse::new(ProtocolVersion::V1).agent_capabilities(capabilities)
        };
        let both = AgentCapabilities::new()
            .load_session(true)
            .session_capabilities(
                SessionCapabilities::new().resume(SessionResumeCapabilities::new()),
            );
        assert_eq!(resume_method(&answer(both)), Some(ResumeMethod::Load));
        let resume = AgentCapabilities::new().session_capabilities(
            SessionCapabilities::new().resume(SessionResumeCapabilities::new()),
        );
        assert_eq!(resume_method(&answer(resume)), Some(ResumeMethod::Resume));
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
        let sent = serde_json::to_value(prompt(&SessionId::new("s"), &message, None)).unwrap();
        let blocks = sent["prompt"].as_array().unwrap();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0]["type"], "text");
        assert_eq!(blocks[0]["text"], "The button overlaps");
        assert_eq!(blocks[1]["type"], "image");
        assert_eq!(blocks[1]["mimeType"], "image/png");
        assert_eq!(blocks[1]["data"], STANDARD.encode(&shot.data));
        let line = format!("[image: image-{}]", image_key(&shot.data));
        assert_eq!(message.shown(), format!("The button overlaps\n{line}"));

        let only = Prompt {
            text: String::new(),
            images: vec![shot],
        };
        let sent = serde_json::to_value(prompt(&SessionId::new("s"), &only, None)).unwrap();
        assert_eq!(sent["prompt"][0]["type"], "image", "no empty text block");
        assert_eq!(only.shown(), line);
    }

    fn png(name: &str, len: usize) -> Upload {
        let mut data = b"\x89PNG\r\n\x1a\n".to_vec();
        data.resize(len.max(data.len()), 0);
        Upload {
            name: name.into(),
            mime: "image/png".into(),
            data,
        }
    }

    #[test]
    fn the_node_refuses_images_the_server_would() {
        let text = || "look".to_owned();
        let ok = Prompt::with_images(text(), vec![png("a.png", 100)]).unwrap();
        assert_eq!(ok.images.len(), 1);

        let many = (0..=beep_proto::MAX_AGENT_IMAGES)
            .map(|i| png(&format!("{i}.png"), 10))
            .collect();
        let refused = Prompt::with_images(text(), many).unwrap_err();
        assert!(refused.starts_with("At most"), "{refused}");

        let big = png("big.png", beep_proto::MAX_AGENT_IMAGE + 1);
        let refused = Prompt::with_images(text(), vec![big]).unwrap_err();
        assert!(refused.starts_with("big.png is over"), "{refused}");

        let mut fake = png("fake.png", 10);
        fake.mime = "image/jpeg".into();
        let refused = Prompt::with_images(text(), vec![fake]).unwrap_err();
        assert!(refused.contains("not the image/jpeg"), "{refused}");

        let mut svg = png("x.svg", 10);
        svg.mime = "image/svg+xml".into();
        assert!(Prompt::with_images(text(), vec![svg]).is_err());
    }

    fn option(id: &str, kind: OptionKind) -> AnswerOption {
        AnswerOption {
            id: id.into(),
            name: id.into(),
            kind,
        }
    }

    fn allowed(options: &[AnswerOption]) -> Option<&str> {
        allow_option(options).map(|option| option.id.as_str())
    }

    #[test]
    fn a_permission_request_is_allowed_for_good_when_it_can_be() {
        use OptionKind::*;
        // Codex, OpenCode and Gemini CLI: once, always, reject.
        let request = [
            option("approved", AllowOnce),
            option("approved-for-session", AllowAlways),
            option("abort", RejectOnce),
        ];
        assert_eq!(allowed(&request), Some("approved-for-session"));
        // Claude Code's prompt for a safety check has no "always".
        let request = [
            option("allow-once", AllowOnce),
            option("reject", RejectOnce),
        ];
        assert_eq!(allowed(&request), Some("allow-once"));
        // A request with no way to allow is still asked.
        let request = [option("reject", RejectOnce)];
        assert_eq!(allowed(&request), None);
    }

    #[test]
    fn plan_approval_keeps_the_context() {
        use OptionKind::*;
        // Claude Code's ExitPlanMode after a session in bypassPermissions.
        let request = [
            option("exit-plan-clear-bypass", AllowAlways),
            option("exit-plan-bypass", AllowAlways),
            option("exit-plan-auto", AllowAlways),
            option("exit-plan-default", AllowOnce),
            option("reject", RejectOnce),
        ];
        assert_eq!(allowed(&request), Some("exit-plan-bypass"));
    }

    #[test]
    fn a_choice_with_nothing_to_reject_is_left_to_the_user() {
        use OptionKind::*;
        // pi's extension select: every choice is an allow_once option.
        let choice = [option("choice-0", AllowOnce), option("choice-1", AllowOnce)];
        assert_eq!(allowed(&choice), None);
        assert_eq!(allowed(&[]), None);
    }

    #[test]
    fn session_models_are_read_from_session_new() {
        let answer = serde_json::json!({
            "sessionId": "s1",
            "models": {
                "availableModels": [
                    {"modelId": "default", "name": "Default (recommended)", "description": "Use the default model"},
                    {"modelId": "opus"},
                ],
                "currentModelId": "default",
            },
        });
        let (created, offered) = session_created(answer).unwrap();
        assert_eq!(created.session_id, SessionId::new("s1"));
        assert_eq!(offered.current.as_deref(), Some("default"));
        assert_eq!(offered.switch, Some(ModelSwitch::SetModel));
        assert_eq!(
            offered.models,
            [
                ModelOption {
                    id: "default".into(),
                    name: "Default (recommended)".into(),
                    description: Some("Use the default model".into()),
                },
                ModelOption {
                    id: "opus".into(),
                    name: "opus".into(),
                    description: None,
                },
            ]
        );
        let request = switch_model(&SessionId::new("s1"), &ModelSwitch::SetModel, "opus").unwrap();
        assert_eq!(request.method(), "session/set_model");
        assert_eq!(
            request.params(),
            &serde_json::json!({"sessionId": "s1", "modelId": "opus"})
        );
    }

    #[test]
    fn a_model_config_option_wins_over_session_models() {
        let answer = serde_json::json!({
            "sessionId": "s1",
            "configOptions": [
                {"id": "mode", "name": "Mode", "category": "mode", "type": "select",
                 "currentValue": "ask", "options": [{"value": "ask", "name": "Ask"}]},
                {"id": "model", "name": "Model", "category": "model", "type": "select",
                 "currentValue": "gpt-test", "options": [
                    {"group": "openai", "name": "OpenAI", "options": [
                        {"value": "gpt-test", "name": "GPT Test"},
                        {"value": "gpt-mini", "name": "GPT Mini", "description": "Faster"}]}]},
            ],
            "models": {"availableModels": [{"modelId": "other"}], "currentModelId": "other"},
        });
        let offered = offered_models(&answer);
        assert_eq!(offered.current.as_deref(), Some("gpt-test"));
        let ids: Vec<&str> = offered.models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["gpt-test", "gpt-mini"]);
        assert_eq!(offered.models[1].description.as_deref(), Some("Faster"));
        let switch = offered.switch.expect("a switch");
        assert_eq!(
            switch,
            ModelSwitch::ConfigOption(SessionConfigId::new("model"))
        );
        let request = switch_model(&SessionId::new("s1"), &switch, "gpt-mini").unwrap();
        assert_eq!(request.method(), "session/set_config_option");
        assert_eq!(request.params()["configId"], "model");
        assert_eq!(request.params()["value"], "gpt-mini");
    }

    #[test]
    fn the_thinking_option_is_read_from_session_new() {
        let answer = serde_json::json!({
            "sessionId": "s1",
            "configOptions": [
                {"id": "model", "name": "Model", "category": "model", "type": "select",
                 "currentValue": "m", "options": [{"value": "m", "name": "M"}]},
                {"id": "reasoning_effort", "name": "Reasoning effort", "category": "thought_level",
                 "type": "select", "currentValue": "medium", "options": [
                    {"value": "low", "name": "Low"}, {"value": "medium", "name": "Medium"}]},
            ],
        });
        let (_, offered) = session_created(answer).unwrap();
        let id = offered.thinking.expect("a thinking option");
        assert_eq!(id, SessionConfigId::new("reasoning_effort"));
        let request = set_effort(&SessionId::new("s1"), &id, Effort::Xhigh).unwrap();
        assert_eq!(request.method(), "session/set_config_option");
        assert_eq!(request.params()["configId"], "reasoning_effort");
        assert_eq!(request.params()["value"], "xhigh");
        let without = serde_json::json!({"sessionId": "s1"});
        assert_eq!(session_created(without).unwrap().1.thinking, None);
    }

    /// Codex's answer to `session/new` and `session/load`, trimmed: its
    /// model selector sits among categories the crate does not name.
    fn codex_answer() -> Value {
        serde_json::json!({
            "sessionId": "s1",
            "models": {
                "availableModels": [
                    {"modelId": "gpt-test[low]", "name": "Test (low)"},
                    {"modelId": "gpt-test[high]", "name": "Test (high)"},
                ],
                "currentModelId": "gpt-test[high]",
            },
            "modes": {"currentModeId": "agent", "availableModes": [{"id": "agent", "name": "Agent"}]},
            "configOptions": [
                {"id": "mode", "name": "Mode", "category": "mode", "type": "select",
                 "currentValue": "agent", "options": [{"value": "agent", "name": "Agent"}]},
                {"id": "collaboration_mode", "name": "Collaboration mode",
                 "category": "collaboration_mode", "type": "select", "currentValue": "default",
                 "options": [{"value": "default", "name": "Default"}]},
                {"id": "model", "name": "Model", "category": "model", "type": "select",
                 "currentValue": "gpt-test", "options": [
                    {"value": "gpt-test", "name": "Test"},
                    {"value": "gpt-mini", "name": "Mini", "description": "Faster"}]},
                {"id": "reasoning_effort", "name": "Reasoning effort", "category": "thought_level",
                 "type": "select", "currentValue": "high", "options": [
                    {"value": "low", "name": "Low"}, {"value": "high", "name": "High"}]},
                {"id": "fast-mode", "name": "Fast mode", "category": "model_config",
                 "type": "select", "currentValue": "off", "options": [{"value": "off", "name": "Off"}]},
            ],
        })
    }

    #[test]
    fn codex_offers_its_model_selector() {
        let (_, offered) = session_created(codex_answer()).unwrap();
        assert_eq!(
            offered.switch,
            Some(ModelSwitch::ConfigOption(SessionConfigId::new("model")))
        );
        assert_eq!(offered.current.as_deref(), Some("gpt-test"));
        assert_eq!(offered.models.len(), 2);
        assert_eq!(
            offered.thinking,
            Some(SessionConfigId::new("reasoning_effort"))
        );
        // `session/load` answers the same without a `sessionId`.
        let mut loaded = codex_answer();
        loaded.as_object_mut().unwrap().remove("sessionId");
        assert_eq!(offers(&loaded), offered);
    }

    #[test]
    fn an_agent_without_models_offers_none() {
        let offered = offered_models(&serde_json::json!({"sessionId": "s1"}));
        assert_eq!(offered, Offered::default());
        // A broken `models` is ignored rather than failing the session.
        let broken = serde_json::json!({"sessionId": "s1", "models": {"availableModels": 3}});
        assert_eq!(offered_models(&broken), Offered::default());
        assert!(session_created(broken).is_ok());
    }

    #[test]
    fn enums_read_as_their_wire_names() {
        use agent_client_protocol::schema::v1::StopReason;
        assert_eq!(wire_name(&StopReason::EndTurn), "end_turn");
        assert_eq!(wire_name(&StopReason::MaxTurnRequests), "max_turn_requests");
    }
}
