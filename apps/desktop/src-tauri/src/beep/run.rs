//! Runs: sending a prompt with what is attached, answering an approval,
//! and streaming a run's events to the bar (`BarEvent`) and live mode
//! (`RunNews`).

use crate::{
    beep::Beep,
    chat::{
        history::{SUGGEST_REPLIES, SentFile, SentImage, suggested_replies, tool_step},
        markdown,
    },
    error::{DesktopError, Result},
    voice::news::Teller,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use beep_proto::{
    ActiveRun, AnswerApproval, ApprovalDecision, ApprovalField, Chart, Chat, DeleteTurnsFrom,
    DeviceAnswer, DeviceQuery, DeviceResult, Event, GetEvents, GetSession, HistoryEntry,
    MAX_UPLOAD, Platform, ReasoningLevel, RunEvent, Upload, WebSource, client::Replies,
};
use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use uuid::Uuid;

const NOT_HERE: &str = "not available on desktop";
/// Images one message may carry, as many as the server takes.
pub const MAX_IMAGES: usize = 8;

/// An admin's one-off model and thinking for a single turn. Both empty
/// sends the turn on the account's own pick.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Pick {
    pub model: Option<String>,
    pub thinking: Option<ReasoningLevel>,
}

/// Where the prompt of a run the bar follows was sent from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Asked {
    Here,
    /// On another device, which answers the run's device queries itself.
    Elsewhere,
}

/// What the bar draws while a run streams.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BarEvent {
    Started {
        run_id: String,
        session_id: String,
    },
    /// The reply so far, as sanitized HTML.
    Text {
        html: String,
    },
    Reasoning {
        delta: String,
    },
    ToolStarted {
        call_id: String,
        name: String,
        description: String,
    },
    ToolResult {
        call_id: String,
        name: String,
        success: bool,
        file: Option<SentFile>,
        /// A `send_image` result's pictures.
        images: Vec<SentImage>,
        /// A `show_products` result's cards, as HTML.
        products: Option<String>,
        /// A `show_cards` result's cards, as HTML.
        cards: Option<String>,
        /// A `show_chart` or `show_stock_chart` result's chart, boxed to
        /// keep the event small.
        chart: Option<Box<Chart>>,
        /// A `code_agent` result's text, which names the session the bar
        /// shows a card for.
        code_agent: Option<String>,
    },
    /// The web pages the reply used, for the chips under it.
    Sources {
        sources: Vec<WebSource>,
    },
    /// Follow-ups to offer under the reply.
    Replies {
        replies: Vec<String>,
    },
    Approval {
        query_id: String,
        app: String,
        action: String,
        fields: Vec<Field>,
    },
    /// Jev picked the model and thinking for this prompt.
    AutoPicked {
        model: String,
        thinking: ReasoningLevel,
    },
    /// What the user said, once the server transcribed a spoken message.
    Transcript {
        text: String,
    },
    Error {
        message: String,
    },
    Finished {
        status: String,
    },
}

/// An image the bar attaches to a message, as a `data:image/...;base64,`
/// URI.
#[derive(Debug, Clone, Deserialize)]
#[serde(try_from = "String")]
pub struct DataImage(String);

impl TryFrom<String> for DataImage {
    type Error = DesktopError;

    fn try_from(uri: String) -> Result<Self> {
        if !uri.starts_with("data:image/") || !uri.contains(";base64,") {
            return Err(DesktopError::BadRequest("not an image".into()));
        }
        Ok(Self(uri))
    }
}

/// A file other than a picture the bar attaches to a message, its bytes in
/// base64.
#[derive(Debug, Clone, Deserialize)]
pub struct FileUpload {
    pub name: String,
    pub mime: String,
    pub data: String,
}

/// What the user sends: the prompt and what is attached to it.
pub struct Sent {
    pub message: String,
    pub images: Vec<DataImage>,
    pub files: Vec<FileUpload>,
    /// Starts a research in the chat, which must be new.
    pub research: bool,
    /// A spoken message, which the server transcribes into `message`.
    pub voice: Option<Upload>,
    /// Said aloud in live mode.
    pub spoken: bool,
}

/// What live mode hears of a run it sent, to read the reply out as it
/// streams.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunNews {
    /// The run started, with its id, so live mode can cancel it.
    Started(String),
    /// More of the reply's text, as the model writes it.
    Text(String),
    /// A tool step started, with its label.
    Tool(String),
    /// The run waits for the user to allow an action.
    Approval(Asking),
    /// The run, or the stream of it, failed with this message.
    Failed(String),
    /// The run finished with this status, such as `completed`, `waiting`
    /// for an approval, or `cancelled`.
    Finished(String),
}

/// The user's answer to an approval card.
#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    Allow,
    Always,
    Deny,
    /// Allow, and the same action for the rest of the chat.
    Session,
}

impl Decision {
    /// Whether it also allows the action for the rest of the chat.
    pub fn for_session(self) -> bool {
        matches!(self, Self::Session)
    }
}

impl From<Decision> for ApprovalDecision {
    fn from(d: Decision) -> Self {
        match d {
            Decision::Allow | Decision::Session => Self::Allow {},
            Decision::Always => Self::Always {},
            Decision::Deny => Self::Deny {},
        }
    }
}

/// An action waiting for the user's OK, as the orb shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Asking {
    pub query_id: String,
    pub app: String,
    pub action: String,
    pub fields: Vec<Field>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Field {
    pub label: String,
    pub value: String,
}

impl From<ApprovalField> for Field {
    fn from(f: ApprovalField) -> Self {
        Self {
            label: f.label,
            value: f.value,
        }
    }
}

/// The files as `Chat` takes them, or an error past `MAX_UPLOAD` in all.
pub fn uploads(files: Vec<FileUpload>) -> Result<Vec<Upload>> {
    let mut total = 0;
    let mut out = Vec::with_capacity(files.len());
    for file in files {
        let data = STANDARD
            .decode(file.data.as_bytes())
            .map_err(|_| DesktopError::BadRequest(format!("{} could not be read", file.name)))?;
        total += data.len();
        if total > MAX_UPLOAD {
            return Err(DesktopError::BadRequest(format!(
                "files up to {} MB in one message",
                MAX_UPLOAD >> 20
            )));
        }
        out.push(Upload {
            name: file.name,
            mime: file.mime,
            data,
        });
    }
    Ok(out)
}

/// The URIs `Chat` and `SendToGroup` take, or an error past `MAX_IMAGES`.
pub fn image_uris(images: Vec<DataImage>) -> Result<Vec<String>> {
    if images.len() > MAX_IMAGES {
        return Err(DesktopError::BadRequest(format!(
            "up to {MAX_IMAGES} images in one message"
        )));
    }
    Ok(images.into_iter().map(|image| image.0).collect())
}

/// The chat a message goes to: `session` when it names one, else a new one.
pub fn chat_id(session: Option<&str>) -> Uuid {
    session
        .and_then(|s| Uuid::parse_str(s).ok())
        .unwrap_or_else(Uuid::new_v4)
}

/// `session` as the chat id a request takes.
pub fn session_uuid(session: &str) -> Result<Uuid> {
    Uuid::parse_str(session).map_err(|_| DesktopError::BadRequest("bad session".into()))
}

impl Beep {
    /// Sends `sent` to `session_id` and streams the run into `channel`,
    /// leaving the bar's chat as it is. Returns the reply's text.
    pub async fn post(
        &self,
        session_id: Uuid,
        sent: Sent,
        pick: Pick,
        channel: Channel<BarEvent>,
    ) -> Result<String> {
        self.post_heard(session_id, sent, pick, channel, None).await
    }

    /// Like `post`, telling `news` of the reply's text and tool steps as
    /// they come.
    pub async fn post_heard(
        &self,
        session_id: Uuid,
        sent: Sent,
        pick: Pick,
        channel: Channel<BarEvent>,
        news: Option<&Teller>,
    ) -> Result<String> {
        let images = image_uris(sent.images)?;
        let files = uploads(sent.files)?;
        let replies = self
            .stream(Chat {
                session_id,
                request_id: Uuid::new_v4(),
                message: sent.message,
                images,
                model: pick.model,
                thinking: pick.thinking,
                platform: Some(Platform::Macos {}),
                files,
                research: sent.research,
                voice: sent.voice,
                spoken: sent.spoken,
            })
            .await?;
        Ok(self.follow(replies, channel, Asked::Here, news).await)
    }

    /// Sends the session's `index`th prompt again on `pick`, after removing
    /// it and every turn after it. Pictures and files sent with it are not
    /// resent.
    pub async fn rerun(
        &self,
        session: String,
        index: usize,
        pick: Pick,
        channel: Channel<BarEvent>,
    ) -> Result<()> {
        let entries = self
            .call(GetSession {
                id: session.clone(),
            })
            .await?;
        let Some((timestamp, message)) = entries
            .into_iter()
            .filter_map(|entry| match entry {
                HistoryEntry::User {
                    content, timestamp, ..
                } => Some((timestamp, content)),
                _ => None,
            })
            .nth(index)
        else {
            return Err(DesktopError::BadRequest(
                "that prompt is no longer in the chat".into(),
            ));
        };
        self.call(DeleteTurnsFrom {
            session_id: session.clone(),
            timestamp,
        })
        .await?;
        let sent = Sent {
            message,
            images: Vec::new(),
            files: Vec::new(),
            research: false,
            voice: None,
            spoken: false,
        };
        self.post(chat_id(Some(&session)), sent, pick, channel)
            .await?;
        Ok(())
    }

    /// Answers an approval card and returns the id of the run it resumes.
    pub async fn approve(
        &self,
        session: &str,
        query_id: String,
        decision: Decision,
    ) -> Result<String> {
        let run = self
            .call(AnswerApproval {
                session_id: session_uuid(session)?,
                query_id,
                decision: decision.into(),
                for_session: decision.for_session(),
            })
            .await?;
        Ok(run.id)
    }

    /// Streams run `run_id`, resumed by an answer given here, into
    /// `channel`, telling `news` too when live mode reads it out.
    pub async fn follow_answered(
        &self,
        run_id: String,
        channel: Channel<BarEvent>,
        news: Option<&Teller>,
    ) -> Result<()> {
        let replies = self.stream(GetEvents { run_id, after: 0 }).await?;
        self.follow(replies, channel, Asked::Here, news).await;
        Ok(())
    }

    /// The run going on in `session`, started here or on another device.
    pub async fn active_run(&self, session: &str) -> Result<Option<String>> {
        let session_id = session_uuid(session)?;
        Ok(self.call(ActiveRun { session_id }).await?.map(|run| run.id))
    }

    /// Streams run `run_id` from its start, for a chat that another device
    /// asked in.
    pub async fn follow_run(&self, run_id: String, channel: Channel<BarEvent>) -> Result<()> {
        let replies = self.stream(GetEvents { run_id, after: 0 }).await?;
        self.follow(replies, channel, Asked::Elsewhere, None).await;
        Ok(())
    }

    /// Streams the run into `channel` until it finishes, and returns the
    /// reply's text.
    async fn follow(
        &self,
        mut replies: Replies<Event>,
        channel: Channel<BarEvent>,
        asked: Asked,
        news: Option<&Teller>,
    ) -> String {
        // Telling never waits, so live mode's reader never holds the bar up.
        let tell = |item: RunNews| {
            if let Some(news) = news {
                news.tell(item);
            }
        };
        let mut text = String::new();
        let mut run_id = String::new();
        let mut seq = 0;
        loop {
            let event = match replies.next().await {
                Ok(Some(event)) => {
                    seq = event.seq;
                    event.event
                }
                // The server ends a stream after a while; a run still going
                // is picked up again from the last event.
                Ok(None) if !run_id.is_empty() => {
                    let more = GetEvents {
                        run_id: run_id.clone(),
                        after: seq,
                    };
                    match self.stream(more).await {
                        Ok(more) => {
                            replies = more;
                            continue;
                        }
                        Err(e) => {
                            let message = e.to_string();
                            tell(RunNews::Failed(message.clone()));
                            // The bar may have closed; the run ends either way.
                            let _ = channel.send(BarEvent::Error { message });
                            break;
                        }
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    let message = self.failed(e).await.to_string();
                    tell(RunNews::Failed(message.clone()));
                    // The bar may have closed; the run ends either way.
                    let _ = channel.send(BarEvent::Error { message });
                    break;
                }
            };
            let out = match event {
                RunEvent::RunStarted {
                    run_id: id,
                    session_id,
                } => {
                    run_id.clone_from(&id);
                    tell(RunNews::Started(id.clone()));
                    BarEvent::Started {
                        run_id: id,
                        session_id,
                    }
                }
                RunEvent::TextDelta { delta } => {
                    text.push_str(&delta);
                    tell(RunNews::Text(delta));
                    BarEvent::Text {
                        html: markdown::to_html(&text),
                    }
                }
                RunEvent::ReasoningDelta { delta } => BarEvent::Reasoning { delta },
                RunEvent::ToolStarted { name, .. } if name == SUGGEST_REPLIES => continue,
                RunEvent::ToolStarted {
                    name,
                    call_id,
                    description,
                } => {
                    tell(RunNews::Tool(description.clone()));
                    BarEvent::ToolStarted {
                        call_id,
                        name,
                        description,
                    }
                }
                RunEvent::ToolResult { name, output, .. } if name == SUGGEST_REPLIES => {
                    match suggested_replies(&output) {
                        Some(replies) => BarEvent::Replies { replies },
                        None => continue,
                    }
                }
                RunEvent::ToolResult {
                    name,
                    call_id,
                    success,
                    output,
                } => {
                    let step = tool_step(&name, &call_id, &output, success);
                    BarEvent::ToolResult {
                        code_agent: step.code_agent,
                        file: step.file,
                        images: step.images,
                        products: step.products,
                        cards: step.cards,
                        chart: step.chart.map(Box::new),
                        call_id,
                        name,
                        success,
                    }
                }
                RunEvent::DeviceQuery { query_id, query } => match query {
                    DeviceQuery::Approve { approval } => {
                        let asking = Asking {
                            query_id,
                            app: approval.app,
                            action: approval.action,
                            fields: approval.fields.into_iter().map(Field::from).collect(),
                        };
                        tell(RunNews::Approval(asking.clone()));
                        BarEvent::Approval {
                            query_id: asking.query_id,
                            app: asking.app,
                            action: asking.action,
                            fields: asking.fields,
                        }
                    }
                    // Another device's run asks that device, not this one.
                    _ if asked == Asked::Elsewhere => continue,
                    // The server leaves phone tools out of desktop runs; this
                    // answers any that slip through.
                    _ => {
                        let answer = DeviceResult {
                            run_id: run_id.clone(),
                            query_id,
                            answer: DeviceAnswer::Err {
                                message: NOT_HERE.into(),
                            },
                        };
                        if let Err(e) = self.call(answer).await {
                            log::warn!("answering a phone query: {e}");
                        }
                        continue;
                    }
                },
                RunEvent::Error { message } => {
                    tell(RunNews::Failed(message.clone()));
                    BarEvent::Error { message }
                }
                RunEvent::RunFinished { status, .. } => {
                    tell(RunNews::Finished(status.clone()));
                    BarEvent::Finished { status }
                }
                RunEvent::AutoPicked { pick } => BarEvent::AutoPicked {
                    model: pick.model,
                    thinking: pick.thinking,
                },
                RunEvent::Transcript { text } => BarEvent::Transcript { text },
                RunEvent::Sources { sources } => BarEvent::Sources { sources },
                RunEvent::Usage { .. } | RunEvent::MemoryScore { .. } => continue,
            };
            let finished = matches!(out, BarEvent::Finished { .. });
            if channel.send(out).is_err() || finished {
                break;
            }
        }
        text
    }
}
