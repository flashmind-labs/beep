//! The connection to the Beep server. One iroh node for the app's life,
//! a connection opened on demand, and the run streams the bar follows.

use crate::{
    cards,
    error::{DesktopError, Result},
    identity::{Identity, secret_key},
    markdown,
    news::Teller,
    products,
    state::{LinkState, NoticeVoice, Saved, StateFile},
    sync::Chats,
    tool_labels,
    work_copy::Copies,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use beep_proto::{
    ActiveRun, AnswerApproval, ApprovalDecision, ApprovalField, Call, Chart, Chat, DeleteTurnsFrom,
    DeviceAnswer, DeviceQuery, DeviceResult, Duplex, Event, GetEvents, GetFile, GetSession,
    GetSessionImages, GetToolLabels, HistoryEntry, ImageRef, ListSessions, MAX_UPLOAD, Notice,
    Platform, ReasoningLevel, RemoveDevice, RunEvent, SearchSessions, SentImages, Session,
    SessionMatch, Stream, TRANSCRIBE_RATE, Transcribe, Transcription, Upload, WebSource,
    client::{Client, Node, OnNotice, Replies, Uplink},
    link::LinkOffer,
};
use iroh::{Endpoint, EndpointId};
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
use tokio::{
    join,
    sync::mpsc,
    time::{Instant, sleep, timeout},
};
use uuid::Uuid;

pub type AsyncMutex<T> = tokio::sync::Mutex<T>;

const CALL_TIMEOUT: Duration = Duration::from_secs(45);
/// How long opening a connection may take, all of it: the dial and the
/// first stream. Without a limit an unreachable server held the client
/// lock, and every call behind it, for good.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const NOT_HERE: &str = "not available on desktop";
/// How long a chat read from the copy waits for its tool labels.
const LABELS_WAIT: Duration = Duration::from_millis(500);
/// Chats remembered as live ones, for the mic in the chat list.
const MAX_LIVE_CHATS: usize = 200;
/// Images one message may carry, as many as the server takes.
pub const MAX_IMAGES: usize = 8;
/// The server saves follow-up replies as this tool's result.
pub const SUGGEST_REPLIES: &str = "suggest_replies";
/// Its result names a file Beep sent, which shows as a card.
const SEND_FILE: &str = "send_file";
/// Its result lists pictures, from the web or kept with the chat.
const SEND_IMAGE: &str = "send_image";
/// Its result lists the picture it made, the same as `send_image`'s.
const IMAGE_GEN: &str = "image_gen";
/// Their results name the video or 3D model they made, the same as
/// `send_file`'s.
const MADE_FILES: [&str; 2] = ["video_gen", "3d_gen"];
/// Their results hold a chart, drawn under the reply.
const SHOW_CHART: &str = "show_chart";
const SHOW_STOCK_CHART: &str = "show_stock_chart";
const GET_ROUTE: &str = "get_route";

/// How long a run's stream is opened again after its connection drops,
/// long enough for a server restart.
const RECONNECT: Duration = Duration::from_secs(90);
/// The first and longest pause between tries to open it.
const FIRST_PAUSE: Duration = Duration::from_millis(500);
const LAST_PAUSE: Duration = Duration::from_secs(5);

/// Where a followed run's stream opens again from.
enum RunSource {
    /// A prompt sent from here, before its run said its id: sent again
    /// with the same request id, which follows the run it started.
    Chat(Box<Chat>),
    /// A run by id.
    Run(String),
}

/// Whether `e` passes once the server is back: the connection dropped or
/// could not open, or the server was restarting.
fn passing(e: &DesktopError) -> bool {
    match e {
        DesktopError::Other(_) => true,
        DesktopError::Api(api) => api.is_restarting(),
        _ => false,
    }
}

/// An admin's one-off model and thinking for a single turn. Both empty
/// sends the turn on the account's own pick.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Pick {
    pub model: Option<String>,
    pub thinking: Option<ReasoningLevel>,
}

/// Runs coding agents; its result describes a session, which the bar shows
/// as a card.
const CODE_AGENT: &str = "code_agent";
/// Its result lists products, which show as a row of cards.
const SHOW_PRODUCTS: &str = "show_products";
/// Its result lists recommendations, which show as a row of cards.
const SHOW_CARDS: &str = "show_cards";

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

/// `at` in milliseconds since the Unix epoch, as the bar's page reads times.
/// A time before the epoch reads as the epoch.
pub fn epoch_ms(at: SystemTime) -> u64 {
    at.duration_since(SystemTime::UNIX_EPOCH)
        .map(|since| u64::try_from(since.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or_default()
}

/// One message of a chat, as the bar shows it.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum Turn {
    User {
        text: String,
        /// When it was sent, in milliseconds: the `message` that
        /// `session_files` lists its files by.
        at: i64,
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
    /// What the step showed while it ran, from `GetToolLabels`. For a step
    /// the server kept none for, what a tool that takes an `action` did,
    /// read from its result.
    pub label: Option<String>,
    /// The call's id, to match the step with its `GetToolLabels` entry.
    #[serde(skip)]
    call_id: String,
    pub success: bool,
    pub file: Option<SentFile>,
    /// A `send_image` result's pictures.
    pub images: Vec<SentImage>,
    /// A `show_products` result's cards, as HTML.
    pub products: Option<String>,
    /// A `show_cards` result's cards, as HTML.
    pub cards: Option<String>,
    /// A `show_chart` or `show_stock_chart` result's chart.
    pub chart: Option<Chart>,
    /// A `get_route` call's id, to fetch its transit card with `GetRoute`.
    pub route: Option<String>,
    /// A `code_agent` result's text, which names the session the bar shows
    /// a card for.
    pub code_agent: Option<String>,
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

/// The chat a message goes to: `session` when it names one, else a new one.
pub fn chat_id(session: Option<&str>) -> Uuid {
    session
        .and_then(|s| Uuid::parse_str(s).ok())
        .unwrap_or_else(Uuid::new_v4)
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
    copies: Copies,
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
                copies: Copies::default(),
            }),
            rx,
        ))
    }

    /// The copies of projects sync keeps on this Mac.
    pub(crate) fn copies(&self) -> &Copies {
        &self.copies
    }

    pub fn id(&self) -> EndpointId {
        self.node.id()
    }

    /// This Mac's iroh endpoint, which the node link shares.
    pub fn endpoint(&self) -> &Endpoint {
        self.node.endpoint()
    }

    pub fn server(&self) -> EndpointId {
        self.server
    }

    pub fn use_mac(&self) -> bool {
        self.saved().uses_mac()
    }

    pub fn set_use_mac(&self, on: bool) {
        self.update_saved(|s| s.set_use_mac(on));
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

    /// The chats started here as researches.
    pub fn researches(&self) -> Vec<Uuid> {
        self.saved().researches.clone()
    }

    /// Remembers that `session` is a research.
    pub fn add_research(&self, session: Uuid) {
        self.add_researches(&[session]);
    }

    /// Remembers chats as researches, saving once.
    pub fn add_researches(&self, sessions: &[Uuid]) {
        let missing = {
            let saved = self.saved();
            sessions.iter().any(|id| !saved.researches.contains(id))
        };
        if !missing {
            return;
        }
        self.update_saved(|s| {
            for session in sessions {
                if !s.researches.contains(session) {
                    s.researches.push(*session);
                }
            }
        });
    }

    /// What a new notice does while the user is here.
    pub fn notice_voice(&self) -> NoticeVoice {
        self.saved().notice_voice()
    }

    pub fn set_notice_voice(&self, choice: NoticeVoice) {
        self.update_saved(|s| s.set_notice_voice(choice));
    }

    /// Whether a pause sends in live mode, rather than only "send".
    pub fn send_on_pause(&self) -> bool {
        self.saved().send_on_pause
    }

    pub fn set_send_on_pause(&self, on: bool) {
        self.update_saved(|s| s.send_on_pause = on);
    }

    /// Whether coding agents notify when they ask or finish a turn.
    pub fn agent_notices(&self) -> bool {
        self.saved().agent_notices
    }

    pub fn set_agent_notices(&self, on: bool) {
        self.update_saved(|s| s.agent_notices = on);
    }

    /// Chats live mode talked in, newest last.
    pub fn live_chats(&self) -> Vec<Uuid> {
        self.saved().live_chats.clone()
    }

    /// Remembers that live mode talked in `session`.
    pub fn add_live_chat(&self, session: Uuid) {
        if self.saved().live_chats.last() == Some(&session) {
            return;
        }
        self.update_saved(|s| {
            s.live_chats.retain(|id| *id != session);
            s.live_chats.push(session);
            let over = s.live_chats.len().saturating_sub(MAX_LIVE_CHATS);
            s.live_chats.drain(..over);
        });
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
        // A closed or revoked client is not kept while a new one is made,
        // so a connect that fails leaves nothing behind.
        *cached = None;
        let client = Arc::new(connect_in_time(self.node.connect(), CONNECT_TIMEOUT).await?);
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
        self.call_within(request, CALL_TIMEOUT).await
    }

    /// `call` for a request that may take longer, such as a clone.
    pub async fn call_within<R: Call>(&self, request: R, limit: Duration) -> Result<R::Reply> {
        let client = self.client().await?;
        match timeout(limit, client.call(&request)).await {
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

    /// Sends a request that keeps sending: what follows goes up the
    /// returned `Uplink`, and the replies come as they arrive.
    pub async fn duplex<R: Duplex>(
        &self,
        request: R,
    ) -> Result<(Uplink<R::Up>, Replies<R::Reply>)> {
        let client = self.client().await?;
        match client.duplex(&request).await {
            Ok(opened) => Ok(opened),
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
        let session_id = Uuid::parse_str(&id).ok();
        if let Some(turns) = session_id.and_then(|id| self.chats.session(id)) {
            // The copy shows at once: the labels only name tool steps
            // better, so a slow or absent server does not hold it back.
            let labels = timeout(LABELS_WAIT, self.tool_labels(session_id))
                .await
                .unwrap_or_else(|_| {
                    log::debug!("tool labels of {id} were slow; showing the chat without them");
                    HashMap::new()
                });
            return Ok(with_labels(turns, labels));
        }
        let (entries, images, labels) = join!(
            self.call(GetSession { id: id.clone() }),
            self.call(GetSessionImages { session_id: id }),
            self.tool_labels(session_id),
        );
        let mut pictures = Pictures::new(images);
        let turns = entries?
            .into_iter()
            .map(|entry| turn(entry, &mut pictures))
            .collect();
        Ok(with_labels(turns, labels))
    }

    /// What each tool step of chat `session_id` showed while it ran, by
    /// call id. Empty from a server that does not keep them, and then the
    /// steps keep the labels read from their results.
    async fn tool_labels(&self, session_id: Option<Uuid>) -> HashMap<String, String> {
        let Some(session_id) = session_id else {
            return HashMap::new();
        };
        match self.call(GetToolLabels { session_id }).await {
            Ok(labels) => labels
                .into_iter()
                .map(|label| (label.call_id, label.description))
                .collect(),
            Err(e) => {
                log::debug!("tool labels of {session_id}: {e}");
                HashMap::new()
            }
        }
    }

    /// Opens a live transcription: audio goes up `Uplink`, words come
    /// back.
    pub async fn transcribe(
        &self,
    ) -> Result<(Uplink<serde_bytes::ByteBuf>, Replies<Transcription>)> {
        let client = self.client().await?;
        let request = Transcribe {
            sample_rate: TRANSCRIBE_RATE,
        };
        match client.duplex(&request).await {
            Ok(opened) => Ok(opened),
            Err(e) => Err(self.failed(e).await),
        }
    }

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
        let chat = Chat {
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
        };
        let from = RunSource::Chat(Box::new(chat));
        let replies = self.reopen(&from, 0, Instant::now() + RECONNECT).await?;
        Ok(self.follow(replies, from, channel, Asked::Here, news).await)
    }

    /// Opens `from`'s stream after event `seq`, again and again while the
    /// server is out of reach or restarting, until `until`.
    async fn reopen(&self, from: &RunSource, seq: i64, until: Instant) -> Result<Replies<Event>> {
        let mut pause = FIRST_PAUSE;
        loop {
            let opened = match from {
                // The same request id follows the run it started, if any.
                RunSource::Chat(chat) => self.stream(chat.as_ref().clone()).await,
                RunSource::Run(run_id) => {
                    let more = GetEvents {
                        run_id: run_id.clone(),
                        after: seq,
                    };
                    self.stream(more).await
                }
            };
            match opened {
                Ok(replies) => return Ok(replies),
                Err(e) if passing(&e) && Instant::now() + pause < until => {
                    log::info!("reconnecting to follow a run: {e}");
                    sleep(pause).await;
                    pause = (pause * 2).min(LAST_PAUSE);
                }
                Err(e) => return Err(e),
            }
        }
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
        let session_id =
            Uuid::parse_str(session).map_err(|_| DesktopError::BadRequest("bad session".into()))?;
        let run = self
            .call(AnswerApproval {
                session_id,
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
        let from = RunSource::Run(run_id);
        let replies = self.reopen(&from, 0, Instant::now() + RECONNECT).await?;
        self.follow(replies, from, channel, Asked::Here, news).await;
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
        let from = RunSource::Run(run_id);
        let replies = self.reopen(&from, 0, Instant::now() + RECONNECT).await?;
        self.follow(replies, from, channel, Asked::Elsewhere, None)
            .await;
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

    /// Streams the run into `channel` until it finishes, and returns the
    /// reply's text. A stream cut off, as a server restart or a network
    /// change does, is opened again from `from` after the last event.
    async fn follow(
        &self,
        mut replies: Replies<Event>,
        mut from: RunSource,
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
        let mut run_id = match &from {
            RunSource::Run(id) => id.clone(),
            RunSource::Chat(_) => String::new(),
        };
        let mut seq = 0;
        // Tries to follow it again end `RECONNECT` after the first that
        // brought no event.
        let mut lost: Option<Instant> = None;
        loop {
            let event = match replies.next().await {
                Ok(Some(event)) => {
                    seq = event.seq;
                    lost = None;
                    event.event
                }
                Ok(None) if run_id.is_empty() => break,
                next => {
                    let reopened = match next {
                        // The server ends a stream after a while; a run
                        // still going is picked up again from the last event.
                        Ok(_) => self.reopen(&from, seq, Instant::now() + RECONNECT).await,
                        // So is one cut off, as by a server restart.
                        Err(e) => match self.failed(e).await {
                            error if passing(&error) => {
                                log::info!("lost a run's stream, following it again: {error}");
                                let until = *lost.get_or_insert_with(Instant::now) + RECONNECT;
                                self.reopen(&from, seq, until).await
                            }
                            error => Err(error),
                        },
                    };
                    match reopened {
                        Ok(more) => {
                            replies = more;
                            continue;
                        }
                        Err(e) => {
                            let message = e.to_string();
                            tell(RunNews::Failed(message.clone()));
                            let _ = channel.send(BarEvent::Error { message });
                            break;
                        }
                    }
                }
            };
            let out = match event {
                RunEvent::RunStarted {
                    run_id: id,
                    session_id,
                } => {
                    run_id.clone_from(&id);
                    from = RunSource::Run(id.clone());
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
                } => BarEvent::ToolResult {
                    code_agent: agent_report(&name, &output, success),
                    file: sent_file(&name, &output),
                    images: sent_images(&name, &output),
                    products: shown_products(&name, &output),
                    cards: shown_cards(&name, &output),
                    chart: shown_chart(&name, &output).map(Box::new),
                    call_id,
                    name,
                    success,
                },
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
        label: tool_labels::action_label(name, output).map(str::to_owned),
        call_id: call_id.to_owned(),
        success,
        file: sent_file(name, output),
        images: sent_images(name, output),
        products: shown_products(name, output),
        cards: shown_cards(name, output),
        chart: shown_chart(name, output),
        code_agent: agent_report(name, output, success),
    }
}

/// The text of a `code_agent` call that went through, or `None` for any
/// other tool or a failed call.
fn agent_report(name: &str, output: &str, success: bool) -> Option<String> {
    (name == CODE_AGENT && success).then(|| output.to_owned())
}

#[derive(Deserialize)]
struct FileOutput {
    file: SentFile,
}

/// The file a `send_file`, `video_gen` or `3d_gen` result names, or `None`
/// for any other tool.
fn sent_file(name: &str, output: &str) -> Option<SentFile> {
    if name != SEND_FILE && !MADE_FILES.contains(&name) {
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

/// The pictures a `send_image` or `image_gen` result lists, or none for any
/// other tool.
fn sent_images(name: &str, output: &str) -> Vec<SentImage> {
    if name != SEND_IMAGE && name != IMAGE_GEN {
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

/// The cards a `show_cards` result lists, or `None` for any other tool.
fn shown_cards(name: &str, output: &str) -> Option<String> {
    if name != SHOW_CARDS {
        return None;
    }
    cards::shown(output)
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

/// `turns` with each tool step labelled as it showed while it ran, where
/// `labels` has its call.
fn with_labels(mut turns: Vec<Turn>, labels: HashMap<String, String>) -> Vec<Turn> {
    if labels.is_empty() {
        return turns;
    }
    for turn in &mut turns {
        let Turn::Assistant { tools, .. } = turn else {
            continue;
        };
        for step in tools {
            if let Some(label) = labels.get(&step.call_id) {
                step.label = Some(label.clone());
            }
        }
    }
    turns
}

fn turn(entry: HistoryEntry, pictures: &mut Pictures) -> Turn {
    match entry {
        HistoryEntry::User {
            content,
            timestamp,
            auto,
        } => Turn::User {
            text: content,
            at: timestamp,
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

/// What `connect` makes, or an error once `limit` passes without it.
async fn connect_in_time<T>(
    connect: impl Future<Output = anyhow::Result<T>>,
    limit: Duration,
) -> Result<T> {
    match timeout(limit, connect).await {
        Ok(connected) => Ok(connected?),
        Err(_) => Err(DesktopError::Other(anyhow::anyhow!(
            "the server did not answer in {} seconds",
            limit.as_secs()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::pending;
    use tokio::time::{Instant, sleep};

    // The clock is paused, so tokio moves it straight to the next timer
    // instead of the test sleeping.
    #[tokio::test(start_paused = true)]
    async fn a_connect_that_never_answers_gives_up_in_time() {
        let started = Instant::now();
        let connect = pending::<anyhow::Result<()>>();
        let error = connect_in_time(connect, CONNECT_TIMEOUT)
            .await
            .expect_err("a server that never answers is an error");
        assert_eq!(started.elapsed(), CONNECT_TIMEOUT);
        assert_eq!(error.to_string(), "the server did not answer in 15 seconds");
    }

    #[tokio::test(start_paused = true)]
    async fn a_connect_inside_the_limit_is_kept() {
        let connect = async {
            sleep(CONNECT_TIMEOUT - Duration::from_secs(1)).await;
            Ok(7)
        };
        let connected = connect_in_time(connect, CONNECT_TIMEOUT).await;
        assert_eq!(connected.expect("it answered in time"), 7);
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_connect_keeps_its_error() {
        let connect = async { Err::<(), _>(anyhow::anyhow!("no route to the server")) };
        let error = connect_in_time(connect, CONNECT_TIMEOUT).await.unwrap_err();
        assert_eq!(error.to_string(), "no route to the server");
    }

    #[test]
    fn with_labels_prefers_the_live_description() {
        let turns = vec![Turn::Assistant {
            html: String::new(),
            tools: vec![
                tool_step("memory", "c1", r#"{"stored":true}"#, true),
                tool_step("memory", "c2", r#"{"stored":true}"#, true),
            ],
            replies: Vec::new(),
            model: String::new(),
        }];
        let labels = HashMap::from([("c1".to_owned(), "Remembering your flight".to_owned())]);
        let Turn::Assistant { tools, .. } = &with_labels(turns, labels)[0] else {
            panic!("an assistant turn stays one");
        };
        assert_eq!(tools[0].label.as_deref(), Some("Remembering your flight"));
        assert_eq!(tools[1].label.as_deref(), Some("Remembering"));
    }

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
    fn made_media_shows_like_sent_media() {
        let image = r#"{"action":"send_image","images":[{"file":{"id":"f3","name":"cat.png","mime":"image/png","size":900},"caption":""}],"provider":"OpenAI","model":"gpt-image-2","path":"cat.png"}"#;
        let images = sent_images("image_gen", image);
        assert_eq!(images.len(), 1);
        assert!(matches!(&images[0].source, ImageSource::File(file) if file.id == "f3"));
        let model = r#"{"action":"send_file","file":{"id":"f4","name":"cat.glb","mime":"model/gltf-binary","size":4000},"provider":"Meshy","model":"latest","path":"cat.glb"}"#;
        for tool in ["video_gen", "3d_gen"] {
            let file = sent_file(tool, model).expect("a made file is named like a sent one");
            assert_eq!(file.id, "f4");
        }
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
