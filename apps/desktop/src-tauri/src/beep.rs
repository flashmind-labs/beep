//! The connection to the Beep server. One iroh node for the app's life,
//! a connection opened on demand, and the run streams the bar follows.

use crate::{
    error::{DesktopError, Result},
    identity::{Identity, secret_key},
    markdown, products,
    state::{LinkState, Saved, StateFile},
    sync::Chats,
};
use beep_proto::{
    ActiveRun, AnswerApproval, ApprovalDecision, ApprovalField, Call, Chart, Chat, DeleteTurnsFrom,
    DeviceAnswer, DeviceQuery, DeviceResult, Event, GetEvents, GetFile, GetSession,
    GetSessionImages, HistoryEntry, ImageRef, ListSessions, Notice, Platform, ReasoningLevel,
    RemoveDevice, RunEvent, SearchSessions, SentImages, Session, SessionMatch, Stream,
    client::{Client, Node, OnNotice, Replies},
    link::LinkOffer,
};
use iroh::EndpointId;
use qrcode::{QrCode, render::svg::Color};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::{Duration, SystemTime},
};
use tauri::ipc::Channel;
use tokio::{join, sync::mpsc, time::timeout};
use uuid::Uuid;

pub type AsyncMutex<T> = tokio::sync::Mutex<T>;

const CALL_TIMEOUT: Duration = Duration::from_secs(45);
const NOT_HERE: &str = "not available on desktop";
/// Images one message may carry, as many as the server takes.
pub const MAX_IMAGES: usize = 8;
/// The server saves follow-up replies as this tool's result.
pub const SUGGEST_REPLIES: &str = "suggest_replies";
/// Its result names a file Beep sent, which shows as a card.
const SEND_FILE: &str = "send_file";
/// Its result lists pictures, from the web or kept with the chat.
const SEND_IMAGE: &str = "send_image";
/// Their results hold a chart, drawn under the reply.
const SHOW_CHART: &str = "show_chart";
const SHOW_STOCK_CHART: &str = "show_stock_chart";
const GET_ROUTE: &str = "get_route";

/// An admin's one-off model and thinking for a single turn. Both empty
/// sends the turn on the account's own pick.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Pick {
    pub model: Option<String>,
    pub thinking: Option<ReasoningLevel>,
}

/// Its result lists products, which show as a row of cards.
const SHOW_PRODUCTS: &str = "show_products";

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
        /// A `show_chart` or `show_stock_chart` result's chart, boxed to
        /// keep the event small.
        chart: Option<Box<Chart>>,
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

/// The URIs `Chat` and `SendToGroup` take, or an error past `MAX_IMAGES`.
pub fn image_uris(images: Vec<DataImage>) -> Result<Vec<String>> {
    if images.len() > MAX_IMAGES {
        return Err(DesktopError::BadRequest(format!(
            "up to {MAX_IMAGES} images in one message"
        )));
    }
    Ok(images.into_iter().map(|image| image.0).collect())
}

#[derive(Debug, Clone, Serialize)]
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

/// One message of a chat, as the bar shows it.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum Turn {
    User {
        text: String,
        /// The pictures sent with it, read with the `picture` command.
        images: Vec<ImageRef>,
        /// The model the turn ran on, empty when the server did not say.
        model: String,
        /// The thinking level, known only when Auto picked it.
        thinking: String,
    },
    Assistant {
        html: String,
        tools: Vec<ToolStep>,
        replies: Vec<String>,
        /// The model that wrote it, empty when the server did not say.
        model: String,
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
    pub file: Option<SentFile>,
    /// A `send_image` result's pictures.
    pub images: Vec<SentImage>,
    /// A `show_products` result's cards, as HTML.
    pub products: Option<String>,
    /// A `show_chart` or `show_stock_chart` result's chart.
    pub chart: Option<Chart>,
    /// A `get_route` call's id, to fetch its transit card with `GetRoute`.
    pub route: Option<String>,
}

/// A file Beep sent in the chat, fetched with `GetFile` when opened.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentFile {
    pub id: String,
    pub name: String,
    pub mime: String,
    pub size: u64,
}

/// A picture Beep sent with `send_image`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentImage {
    #[serde(flatten)]
    pub source: ImageSource,
    #[serde(default)]
    pub caption: String,
}

/// Where a sent picture comes from.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageSource {
    /// An https image on the web.
    Url(String),
    /// An image kept with the chat, fetched like a sent file.
    File(SentFile),
}

/// The pictures a chat's messages carried, by the message they came with:
/// the user entry's timestamp in a session, the message's seq in a group.
pub struct Pictures(HashMap<i64, Vec<ImageRef>>);

impl Pictures {
    /// The pictures listed in `reply`. A server too old to keep pictures
    /// fails the call, which only means there are none.
    pub fn new(reply: Result<Vec<SentImages>>) -> Self {
        let sent = reply.unwrap_or_else(|e| {
            log::info!("listing pictures: {e}");
            Vec::new()
        });
        Self(sent.into_iter().map(|s| (s.message, s.images)).collect())
    }

    /// The pictures `message` carried, if any.
    pub fn take(&mut self, message: i64) -> Vec<ImageRef> {
        self.0.remove(&message).unwrap_or_default()
    }
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
    /// The group the bar shows, so its messages do not also notify.
    group: Mutex<Option<String>>,
    chats: Chats,
}

/// The notices the server pushes, for the app to show.
pub type Notices = mpsc::UnboundedReceiver<Notice>;

impl Beep {
    pub async fn start(
        identity: Identity,
        server: EndpointId,
        file: StateFile,
        chats: Chats,
    ) -> anyhow::Result<(Arc<Self>, Notices)> {
        let (tx, rx) = mpsc::unbounded_channel();
        let on_notice: OnNotice = Arc::new(move |notice| tx.send(notice).is_ok());
        let node = Node::bind(secret_key(&identity), server, on_notice)
            .await?
            .with_app(Platform::Macos {}, env!("CARGO_PKG_VERSION"));
        let saved = file.load();
        Ok((
            Arc::new(Self {
                node,
                server,
                identity,
                client: AsyncMutex::new(None),
                saved: Mutex::new(saved),
                file,
                group: Mutex::new(None),
                chats,
            }),
            rx,
        ))
    }

    pub fn id(&self) -> EndpointId {
        self.node.id()
    }

    /// The chats copied from the server.
    pub fn chats(&self) -> &Chats {
        &self.chats
    }

    fn saved(&self) -> MutexGuard<'_, Saved> {
        self.saved.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Changes the saved state and writes it to disk.
    fn update_saved(&self, f: impl FnOnce(&mut Saved)) {
        let mut saved = self.saved();
        f(&mut saved);
        self.file.save(&saved);
    }

    pub fn link_state(&self) -> LinkState {
        self.saved().link
    }

    pub fn set_link_state(&self, link: LinkState) {
        self.update_saved(|s| s.link = link);
    }

    pub fn session(&self) -> Option<String> {
        self.saved().session.clone()
    }

    pub fn set_session(&self, session: Option<String>) {
        self.update_saved(|s| s.session = session);
    }

    pub fn group(&self) -> Option<String> {
        self.group
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn set_group(&self, group: Option<String>) {
        *self.group.lock().unwrap_or_else(PoisonError::into_inner) = group;
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
        let qr_svg = qr_svg(&uri)?;
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

    pub async fn stream<R: Stream>(&self, request: R) -> Result<Replies<R::Reply>> {
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
        match self.chats.sessions() {
            Some(sessions) => Ok(sessions),
            None => self.call(ListSessions {}).await,
        }
    }

    /// Searches the local copy, or the server before the copy is ready.
    pub async fn search(&self, query: String) -> Result<Vec<SessionMatch>> {
        if let Some(found) = self.chats.search(&query) {
            return Ok(found);
        }
        self.call(SearchSessions { query }).await
    }

    pub async fn history(&self, id: String) -> Result<Vec<Turn>> {
        if let Some(turns) = Uuid::parse_str(&id)
            .ok()
            .and_then(|id| self.chats.session(id))
        {
            return Ok(turns);
        }
        let (entries, images) = join!(
            self.call(GetSession { id: id.clone() }),
            self.call(GetSessionImages { session_id: id }),
        );
        let mut pictures = Pictures::new(images);
        Ok(entries?
            .into_iter()
            .map(|entry| turn(entry, &mut pictures))
            .collect())
    }

    /// Sends `message` and streams the run into `channel`. Returns the
    /// session the message went to.
    pub async fn send(
        &self,
        session: Option<String>,
        message: String,
        images: Vec<DataImage>,
        pick: Pick,
        channel: Channel<BarEvent>,
    ) -> Result<String> {
        let images = image_uris(images)?;
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
                images,
                model: pick.model,
                thinking: pick.thinking,
                platform: Some(Platform::Macos {}),
            })
            .await?;
        self.follow(replies, channel).await;
        Ok(session_id.to_string())
    }

    /// Sends the session's `index`th prompt again on `pick`, after removing
    /// it and every turn after it. Pictures sent with it are not resent.
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
        self.send(Some(session), message, Vec::new(), pick, channel)
            .await?;
        Ok(())
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

    /// The run going on in `session`, started here or on another device.
    pub async fn active_run(&self, session: &str) -> Result<Option<String>> {
        let session_id =
            Uuid::parse_str(session).map_err(|_| DesktopError::BadRequest("bad session".into()))?;
        Ok(self.call(ActiveRun { session_id }).await?.map(|run| run.id))
    }

    /// Streams run `run_id` from its start, for a chat that another device
    /// asked in.
    pub async fn follow_run(&self, run_id: String, channel: Channel<BarEvent>) -> Result<()> {
        let replies = self.stream(GetEvents { run_id, after: 0 }).await?;
        self.follow(replies, channel).await;
        Ok(())
    }

    /// Downloads a sent file to `dest`, part by part. It lands under a
    /// `.part` name first, so a cut-off download is never opened.
    pub async fn download(&self, id: &str, dest: &Path) -> Result<()> {
        let mut partial = dest.as_os_str().to_owned();
        partial.push(".part");
        let partial = PathBuf::from(partial);
        let mut result = self.download_to(id, &partial).await;
        if result.is_ok() {
            result = fs::rename(&partial, dest).map_err(|e| anyhow::Error::from(e).into());
        }
        if result.is_err() {
            let _ = fs::remove_file(&partial);
        }
        result
    }

    /// Writes every part of file `id` to `path`.
    async fn download_to(&self, id: &str, path: &Path) -> Result<()> {
        let mut out = File::create(path).map_err(anyhow::Error::from)?;
        let mut offset = 0;
        loop {
            let part = self
                .call(GetFile {
                    id: id.to_owned(),
                    offset,
                })
                .await?;
            if part.data.is_empty() && offset < part.size {
                return Err(DesktopError::Other(anyhow::anyhow!(
                    "the download stopped at {offset} of {} bytes",
                    part.size
                )));
            }
            out.write_all(&part.data).map_err(anyhow::Error::from)?;
            offset += part.data.len() as u64;
            if offset >= part.size {
                return Ok(());
            }
        }
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
                    output,
                } => BarEvent::ToolResult {
                    file: sent_file(&name, &output),
                    images: sent_images(&name, &output),
                    products: shown_products(&name, &output),
                    chart: shown_chart(&name, &output).map(Box::new),
                    call_id,
                    name,
                    success,
                },
                RunEvent::DeviceQuery { query_id, query } => match query {
                    DeviceQuery::Approve { approval } => BarEvent::Approval {
                        query_id,
                        app: approval.app,
                        action: approval.action,
                        fields: approval.fields.into_iter().map(Field::from).collect(),
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
                RunEvent::AutoPicked { pick } => BarEvent::AutoPicked {
                    model: pick.model,
                    thinking: pick.thinking,
                },
                RunEvent::Usage { .. } | RunEvent::MemoryScore { .. } => continue,
            };
            if channel.send(out).is_err() {
                break;
            }
        }
    }
}

#[derive(Deserialize)]
struct Suggestions {
    replies: Vec<String>,
}

/// The replies in a `suggest_replies` result, or `None` when it has none.
pub fn suggested_replies(output: &str) -> Option<Vec<String>> {
    let suggestions: Suggestions = serde_json::from_str(output).ok()?;
    let replies: Vec<String> = suggestions
        .replies
        .into_iter()
        .filter(|reply| !reply.trim().is_empty())
        .collect();
    (!replies.is_empty()).then_some(replies)
}

/// A tool result as a step of a reply.
pub fn tool_step(name: &str, call_id: &str, output: &str, success: bool) -> ToolStep {
    ToolStep {
        route: (name == GET_ROUTE && success && !call_id.is_empty()).then(|| call_id.to_owned()),
        name: name.to_owned(),
        success,
        file: sent_file(name, output),
        images: sent_images(name, output),
        products: shown_products(name, output),
        chart: shown_chart(name, output),
    }
}

#[derive(Deserialize)]
struct FileOutput {
    file: SentFile,
}

/// The file a `send_file` result names, or `None` for any other tool.
fn sent_file(name: &str, output: &str) -> Option<SentFile> {
    if name != SEND_FILE {
        return None;
    }
    serde_json::from_str::<FileOutput>(output)
        .ok()
        .map(|o| o.file)
}

#[derive(Deserialize)]
struct ImagesOutput {
    images: Vec<SentImage>,
}

/// The pictures a `send_image` result lists, or none for any other tool.
fn sent_images(name: &str, output: &str) -> Vec<SentImage> {
    if name != SEND_IMAGE {
        return Vec::new();
    }
    serde_json::from_str::<ImagesOutput>(output)
        .map(|o| o.images)
        .unwrap_or_default()
}

/// The cards a `show_products` result lists, or `None` for any other tool.
fn shown_products(name: &str, output: &str) -> Option<String> {
    if name != SHOW_PRODUCTS {
        return None;
    }
    products::shown(output)
}

#[derive(Deserialize)]
struct ChartOutput {
    chart: Chart,
}

/// The chart a `show_chart` or `show_stock_chart` result draws, or `None`
/// for any other tool.
fn shown_chart(name: &str, output: &str) -> Option<Chart> {
    if name != SHOW_CHART && name != SHOW_STOCK_CHART {
        return None;
    }
    serde_json::from_str::<ChartOutput>(output)
        .ok()
        .map(|o| o.chart)
        .filter(|chart| chart.series.iter().any(|s| !s.points.is_empty()))
}

/// `data` as a QR code the bar can draw.
pub fn qr_svg(data: &str) -> Result<String> {
    let qr = QrCode::new(data.as_bytes()).map_err(|e| DesktopError::BadRequest(e.to_string()))?;
    Ok(qr
        .render::<Color>()
        .quiet_zone(false)
        .dark_color(Color("#0A0E0C"))
        .light_color(Color("#F4F8F5"))
        .build())
}

fn turn(entry: HistoryEntry, pictures: &mut Pictures) -> Turn {
    match entry {
        HistoryEntry::User {
            content,
            timestamp,
            auto,
        } => Turn::User {
            text: content,
            images: pictures.take(timestamp),
            model: auto
                .as_ref()
                .map(|pick| pick.model.clone())
                .unwrap_or_default(),
            thinking: auto
                .map(|pick| pick.thinking.to_string())
                .unwrap_or_default(),
        },
        HistoryEntry::Assistant {
            content,
            tools,
            model,
            ..
        } => {
            let (suggest, tools): (Vec<_>, Vec<_>) =
                tools.into_iter().partition(|t| t.name == SUGGEST_REPLIES);
            Turn::Assistant {
                html: markdown::to_html(&content),
                tools: tools
                    .iter()
                    .map(|t| tool_step(&t.name, &t.call_id, &t.output, t.success))
                    .collect(),
                replies: suggest
                    .iter()
                    .find_map(|t| suggested_replies(&t.output))
                    .unwrap_or_default(),
                model,
            }
        }
        HistoryEntry::Scheduled { label, .. } => Turn::Scheduled { label },
        HistoryEntry::Approval { query_id, approval } => Turn::Approval {
            query_id,
            app: approval.app,
            action: approval.action,
            fields: approval.fields.into_iter().map(Field::from).collect(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sent_file_reads_send_file_results_only() {
        let output = r#"{"action":"send_file","file":{"id":"f1","name":"Pass.pdf","mime":"application/pdf","size":26000}}"#;
        let file = sent_file(SEND_FILE, output).expect("a send_file result names its file");
        assert_eq!(file.id, "f1");
        assert_eq!(file.size, 26000);
        assert!(sent_file("web_search", output).is_none());
        assert!(sent_file(SEND_FILE, "failed").is_none());
    }

    #[test]
    fn sent_images_reads_urls_and_kept_files() {
        let output = r#"{"action":"send_image","images":[{"url":"https://pics.example/a.jpg","caption":"A"},{"file":{"id":"f2","name":"chart.png","mime":"image/png","size":900},"caption":""}],"failed":[]}"#;
        let images = sent_images(SEND_IMAGE, output);
        assert_eq!(images.len(), 2);
        assert!(
            matches!(&images[0].source, ImageSource::Url(url) if url == "https://pics.example/a.jpg")
        );
        assert_eq!(images[0].caption, "A");
        assert!(matches!(&images[1].source, ImageSource::File(file) if file.id == "f2"));
        assert!(sent_images(SEND_FILE, output).is_empty());
        assert!(sent_images(SEND_IMAGE, "failed").is_empty());
    }

    #[test]
    fn shown_products_reads_show_products_results_only() {
        let output = r#"{"action":"show_products","products":[{"title":"Kettle","url":"https://shop.example/k","store":"shop.example","price":19.5,"currency":"USD"}]}"#;
        let row = shown_products(SHOW_PRODUCTS, output).expect("a show_products result has cards");
        assert!(row.contains("<b>$19.50</b>"), "{row}");
        assert!(shown_products(SEND_FILE, output).is_none());
        assert!(shown_products(SHOW_PRODUCTS, "failed").is_none());
    }

    #[test]
    fn shown_chart_reads_chart_results_only() {
        let output = r#"{"action":"show_stock_chart","chart":{"title":"AAPL","kind":"area","x":"time","categories":[],"unit":"USD","series":[{"name":"AAPL","points":[{"x":1.0,"y":2.0}]}],"stock":null,"source":"Yahoo Finance"}}"#;
        let chart = shown_chart(SHOW_STOCK_CHART, output).expect("a chart result has a chart");
        assert_eq!(chart.series[0].points.len(), 1);
        assert!(shown_chart(SHOW_PRODUCTS, output).is_none());
        assert!(shown_chart(SHOW_CHART, "failed").is_none());
    }
}
