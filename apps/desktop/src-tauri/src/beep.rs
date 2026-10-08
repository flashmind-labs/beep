//! The connection to the Beep server. One iroh node for the app's life,
//! a connection opened on demand, and the run streams the bar follows.

use crate::{
    error::{DesktopError, Result},
    identity::{Identity, secret_key},
    markdown,
    state::{LinkState, Saved, StateFile},
};
use beep_proto::{
    AnswerApproval, ApprovalDecision, Call, CancelRun, Chat, DeviceAnswer, DeviceQuery,
    DeviceResult, Event, GetEvents, GetSession, HistoryEntry, ListSessions, Notice, RemoveDevice,
    RunEvent, SearchSessions, Session, SessionMatch, Stream,
    client::{Client, Node, OnNotice, Replies},
    link::LinkOffer,
};
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use std::{
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, SystemTime},
};
use tauri::ipc::Channel;
use tokio::{sync::mpsc, time::timeout};
use uuid::Uuid;

pub type AsyncMutex<T> = tokio::sync::Mutex<T>;

const CALL_TIMEOUT: Duration = Duration::from_secs(45);
const NOT_HERE: &str = "not available on desktop";
/// The server saves follow-up replies as this tool's result.
const SUGGEST_REPLIES: &str = "suggest_replies";

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
    Error {
        message: String,
    },
    Finished {
        status: String,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct Field {
    pub label: String,
    pub value: String,
}

/// One message of a chat, as the bar shows it.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum Turn {
    User {
        text: String,
    },
    Assistant {
        html: String,
        tools: Vec<ToolStep>,
        replies: Vec<String>,
    },
    Scheduled {
        label: String,
    },
    Approval {
        query_id: String,
        app: String,
        action: String,
        fields: Vec<Field>,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolStep {
    pub name: String,
    pub success: bool,
}

/// A code for the phone to scan.
#[derive(Debug, Clone, Serialize)]
pub struct LinkView {
    pub uri: String,
    pub code: String,
    pub qr_svg: String,
    pub expires_in: u64,
}

/// The user's answer to an approval card, and where to stream the run it
/// resumes.
pub struct Answer {
    pub session: String,
    pub query_id: String,
    pub decision: ApprovalDecision,
    pub channel: Channel<BarEvent>,
}

pub struct Beep {
    node: Node,
    server: EndpointId,
    identity: Identity,
    client: AsyncMutex<Option<Arc<Client>>>,
    saved: Mutex<Saved>,
    file: StateFile,
}

/// The notices the server pushes, for the app to show.
pub type Notices = mpsc::UnboundedReceiver<Notice>;

impl Beep {
    pub async fn start(
        identity: Identity,
        server: EndpointId,
        file: StateFile,
    ) -> anyhow::Result<(Arc<Self>, Notices)> {
        let (tx, rx) = mpsc::unbounded_channel();
        let on_notice: OnNotice = Arc::new(move |notice| tx.send(notice).is_ok());
        let node = Node::bind(secret_key(&identity), server, on_notice).await?;
        let saved = file.load();
        Ok((
            Arc::new(Self {
                node,
                server,
                identity,
                client: AsyncMutex::new(None),
                saved: Mutex::new(saved),
                file,
            }),
            rx,
        ))
    }

    pub fn id(&self) -> EndpointId {
        self.node.id()
    }

    fn with_saved<T>(&self, f: impl FnOnce(&mut Saved) -> T) -> T {
        let mut saved = self.saved.lock().unwrap_or_else(PoisonError::into_inner);
        let out = f(&mut saved);
        self.file.save(&saved);
        out
    }

    pub fn link_state(&self) -> LinkState {
        self.with_saved(|s| s.link)
    }

    pub fn set_link_state(&self, link: LinkState) {
        self.with_saved(|s| s.link = link);
    }

    pub fn session(&self) -> Option<String> {
        self.with_saved(|s| s.session.clone())
    }

    pub fn set_session(&self, session: Option<String>) {
        self.with_saved(|s| s.session = session);
    }

    /// A fresh offer, signed for this server.
    pub fn link_offer(&self, name: &str) -> Result<LinkView> {
        let offer = LinkOffer::sign(
            &secret_key(&self.identity),
            self.server,
            name,
            SystemTime::now(),
        )
        .map_err(|e| DesktopError::BadRequest(e.to_string()))?;
        let uri = offer.to_uri();
        let qr = qrcode::QrCode::new(uri.as_bytes())
            .map_err(|e| DesktopError::BadRequest(e.to_string()))?;
        let qr_svg = qr
            .render::<qrcode::render::svg::Color>()
            .quiet_zone(false)
            .dark_color(qrcode::render::svg::Color("#0A0E0C"))
            .light_color(qrcode::render::svg::Color("#F4F8F5"))
            .build();
        let expires_in = offer
            .expires_at()
            .duration_since(SystemTime::now())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Ok(LinkView {
            code: offer.code(),
            uri,
            qr_svg,
            expires_in,
        })
    }

    /// The open connection, or a new one. Never connects while unlinked, so
    /// the server does not make this key an account of its own.
    async fn client(&self) -> Result<Arc<Client>> {
        match self.link_state() {
            LinkState::Unlinked => return Err(DesktopError::Unlinked),
            LinkState::Revoked => return Err(DesktopError::Revoked),
            LinkState::Linked => {}
        }
        let mut cached = self.client.lock().await;
        if let Some(client) = cached.as_ref() {
            if client.is_revoked() {
                self.set_link_state(LinkState::Revoked);
                return Err(DesktopError::Revoked);
            }
            if !client.is_closed() {
                return Ok(client.clone());
            }
        }
        let client = Arc::new(self.node.connect().await?);
        *cached = Some(client.clone());
        Ok(client)
    }

    /// Maps a failed request, marking the key revoked when the server said so.
    async fn failed(&self, e: anyhow::Error) -> DesktopError {
        let revoked = self
            .client
            .lock()
            .await
            .as_ref()
            .is_some_and(|c| c.is_revoked());
        if revoked {
            self.set_link_state(LinkState::Revoked);
            return DesktopError::Revoked;
        }
        e.into()
    }

    pub async fn call<R: Call>(&self, request: R) -> Result<R::Reply> {
        let client = self.client().await?;
        match timeout(CALL_TIMEOUT, client.call(&request)).await {
            Ok(Ok(reply)) => Ok(reply),
            Ok(Err(e)) => Err(self.failed(e).await),
            Err(_) => Err(DesktopError::BadRequest("the server took too long".into())),
        }
    }

    async fn stream<R: Stream>(&self, request: R) -> Result<Replies<R::Reply>> {
        let client = self.client().await?;
        match client.stream(&request).await {
            Ok(replies) => Ok(replies),
            Err(e) => Err(self.failed(e).await),
        }
    }

    /// Takes this computer off the account.
    pub async fn unlink(&self) -> Result<()> {
        let id = self.id().to_string();
        self.call(RemoveDevice { id }).await?;
        self.set_link_state(LinkState::Revoked);
        Ok(())
    }

    pub async fn sessions(&self) -> Result<Vec<Session>> {
        self.call(ListSessions {}).await
    }

    pub async fn search(&self, query: String) -> Result<Vec<SessionMatch>> {
        self.call(SearchSessions { query }).await
    }

    pub async fn history(&self, id: String) -> Result<Vec<Turn>> {
        let entries = self.call(GetSession { id }).await?;
        Ok(entries.into_iter().map(turn).collect())
    }

    /// Sends `message` and streams the run into `channel`. Returns the
    /// session the message went to.
    pub async fn send(
        &self,
        session: Option<String>,
        message: String,
        channel: Channel<BarEvent>,
    ) -> Result<String> {
        let session_id = match session.as_deref().map(Uuid::parse_str) {
            Some(Ok(id)) => id,
            _ => Uuid::new_v4(),
        };
        self.set_session(Some(session_id.to_string()));
        let replies = self
            .stream(Chat {
                session_id,
                request_id: Uuid::new_v4(),
                message,
                images: Vec::new(),
                model: None,
                thinking: None,
            })
            .await?;
        self.follow(replies, channel).await;
        Ok(session_id.to_string())
    }

    /// Answers an approval card and streams the run it starts.
    pub async fn answer(&self, answer: Answer) -> Result<()> {
        let Answer {
            session,
            query_id,
            decision,
            channel,
        } = answer;
        let session_id = Uuid::parse_str(&session)
            .map_err(|_| DesktopError::BadRequest("bad session".into()))?;
        let run = self
            .call(AnswerApproval {
                session_id,
                query_id,
                decision,
            })
            .await?;
        let replies = self
            .stream(GetEvents {
                run_id: run.id,
                after: 0,
            })
            .await?;
        self.follow(replies, channel).await;
        Ok(())
    }

    pub async fn cancel(&self, run_id: String) -> Result<()> {
        self.call(CancelRun { id: run_id }).await
    }

    async fn follow(&self, mut replies: Replies<Event>, channel: Channel<BarEvent>) {
        let mut text = String::new();
        let mut run_id = String::new();
        loop {
            let event = match replies.next().await {
                Ok(Some(event)) => event.event,
                Ok(None) => break,
                Err(e) => {
                    let message = self.failed(e).await.to_string();
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
                    BarEvent::Started {
                        run_id: id,
                        session_id,
                    }
                }
                RunEvent::TextDelta { delta } => {
                    text.push_str(&delta);
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
                } => BarEvent::ToolStarted {
                    call_id,
                    name,
                    description,
                },
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
                    ..
                } => BarEvent::ToolResult {
                    call_id,
                    name,
                    success,
                },
                RunEvent::DeviceQuery { query_id, query } => match query {
                    DeviceQuery::Approve { approval } => BarEvent::Approval {
                        query_id,
                        app: approval.app,
                        action: approval.action,
                        fields: fields(approval.fields),
                    },
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
                RunEvent::Error { message } => BarEvent::Error { message },
                RunEvent::RunFinished { status, .. } => BarEvent::Finished { status },
                RunEvent::Usage { .. }
                | RunEvent::MemoryScore { .. }
                | RunEvent::AutoPicked { .. } => continue,
            };
            if channel.send(out).is_err() {
                break;
            }
        }
    }
}

fn fields(fields: Vec<beep_proto::ApprovalField>) -> Vec<Field> {
    fields
        .into_iter()
        .map(|f| Field {
            label: f.label,
            value: f.value,
        })
        .collect()
}

#[derive(Deserialize)]
struct Suggestions {
    replies: Vec<String>,
}

/// The replies in a `suggest_replies` result, or `None` when it has none.
fn suggested_replies(output: &str) -> Option<Vec<String>> {
    let suggestions: Suggestions = serde_json::from_str(output).ok()?;
    let replies: Vec<String> = suggestions
        .replies
        .into_iter()
        .filter(|reply| !reply.trim().is_empty())
        .collect();
    (!replies.is_empty()).then_some(replies)
}

fn turn(entry: HistoryEntry) -> Turn {
    match entry {
        HistoryEntry::User { content, .. } => Turn::User { text: content },
        HistoryEntry::Assistant { content, tools, .. } => {
            let (suggest, tools): (Vec<_>, Vec<_>) =
                tools.into_iter().partition(|t| t.name == SUGGEST_REPLIES);
            Turn::Assistant {
                html: markdown::to_html(&content),
                tools: tools
                    .into_iter()
                    .map(|t| ToolStep {
                        name: t.name,
                        success: t.success,
                    })
                    .collect(),
                replies: suggest
                    .iter()
                    .find_map(|t| suggested_replies(&t.output))
                    .unwrap_or_default(),
            }
        }
        HistoryEntry::Scheduled { label, .. } => Turn::Scheduled { label },
        HistoryEntry::Approval { query_id, approval } => Turn::Approval {
            query_id,
            app: approval.app,
            action: approval.action,
            fields: fields(approval.fields),
        },
    }
}
