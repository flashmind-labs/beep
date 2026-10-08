//! The Beep protocol: messages, framing and client, shared by the server and
//! the Android app.
//!
//! Each request is a struct declared in `requests!` with its op and reply
//! type. Enums made with `wire_enum!` encode as `[op, field...]`; structs
//! encode as their fields in declaration order.
//!
//! With the `uniffi` feature, the types a client sees also derive UniFFI
//! bindings.

pub mod client;
pub mod link;
pub mod names;
pub mod seal;
pub mod wire;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

pub use flashmind_types::ReasoningLevel;
use link::LinkOffer;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{fmt, time::SystemTime};
use uuid::Uuid;

macro_rules! one {
    ($x:tt) => {
        1
    };
}

/// Enum serialized as `[op, field...]` instead of serde's `{"Variant": ...}`.
macro_rules! wire_enum {
    ($(#[$meta:meta])* pub enum $enum:ident {
        $($op:literal $name:ident { $($field:ident: $ty:ty),* $(,)? })*
    }) => {
        $(#[$meta])*
        pub enum $enum {
            $($name { $($field: $ty),* },)*
        }

        impl Serialize for $enum {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                use serde::ser::SerializeTuple;
                match self {
                    $(Self::$name { $($field),* } => {
                        let mut t = s.serialize_tuple(1 $(+ one!($field))*)?;
                        t.serialize_element(&($op as u8))?;
                        $(t.serialize_element($field)?;)*
                        t.end()
                    })*
                }
            }
        }

        impl<'de> Deserialize<'de> for $enum {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                use serde::de::{Error, SeqAccess, Visitor};
                struct V;
                impl<'de> Visitor<'de> for V {
                    type Value = $enum;
                    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                        f.write_str(concat!("[op, field...] for ", stringify!($enum)))
                    }
                    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<$enum, A::Error> {
                        let op: u8 = seq.next_element()?.ok_or_else(|| A::Error::custom("missing op"))?;
                        match op {
                            $($op => Ok($enum::$name {
                                $($field: seq.next_element()?.ok_or_else(|| {
                                    A::Error::custom(concat!("missing ", stringify!($field)))
                                })?),*
                            }),)*
                            _ => Err(A::Error::custom(format_args!("unknown op {op}"))),
                        }
                    }
                }
                d.deserialize_seq(V)
            }
        }
    };
}

/// A request type. Its frame starts with `OP`; the body is the struct's fields.
pub trait Rpc: Serialize + DeserializeOwned + Send + 'static {
    const OP: Op;
}

/// A request answered by exactly one reply.
pub trait Call: Rpc {
    type Reply: Serialize + DeserializeOwned + Send + Sync + 'static;
}

/// A request answered by any number of replies, ended by finishing the stream.
pub trait Stream: Rpc {
    type Reply: Serialize + DeserializeOwned + Send + Sync + 'static;
}

/// Declares every request: its op, its fields and what it replies with.
macro_rules! requests {
    ($($(#[$meta:meta])* $op:literal $name:ident { $($field:ident: $ty:ty),* $(,)? } => $kind:ident<$reply:ty>)*) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[repr(u8)]
        pub enum Op {
            $($name = $op,)*
        }

        impl Op {
            pub const ALL: &[Op] = &[$(Op::$name),*];
        }

        impl TryFrom<u8> for Op {
            type Error = ApiError;

            fn try_from(op: u8) -> Result<Self, ApiError> {
                match op {
                    $($op => Ok(Op::$name),)*
                    _ => Err(ApiError::bad_request(format!("unknown op {op}"))),
                }
            }
        }

        $(
            $(#[$meta])*
            #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
            pub struct $name {
                $(pub $field: $ty,)*
            }

            impl Rpc for $name {
                const OP: Op = Op::$name;
            }

            impl $kind for $name {
                type Reply = $reply;
            }
        )*
    };
}

requests! {
    1 Health {} => Call<()>
    /// Starts a run and streams its events, starting with `RunStarted`. Resending
    /// the same `request_id` follows the existing run instead of starting another.
    /// `images` are `data:image/...;base64,` URIs. `model` is a `ModelChoice::id` to run this
    /// one turn on instead of the picked model, and `thinking` how long it
    /// reasons; only admins may set either.
    2 Chat { session_id: Uuid, request_id: Uuid, message: String, images: Vec<String>, model: Option<String>, thinking: Option<ReasoningLevel> } => Stream<Event>
    3 GetRun { id: String } => Call<Run>
    4 CancelRun { id: String } => Call<()>
    5 GetEvents { run_id: String, after: i64 } => Stream<Event>
    /// Returns whether a run was still waiting for this answer.
    6 DeviceResult { run_id: String, query_id: String, answer: DeviceAnswer } => Call<bool>
    7 ListSessions {} => Call<Vec<Session>>
    8 GetSession { id: String } => Call<Vec<HistoryEntry>>
    9 RenameSession { id: String, title: String } => Call<()>
    10 DeleteSession { id: String } => Call<()>
    11 PinSession { id: String, pinned: bool } => Call<()>
    /// Every memory, newest first, including ones whose time has passed.
    12 ListMemories {} => Call<Vec<Memory>>
    13 DeleteMemory { id: String } => Call<()>
    14 ListCrons {} => Call<Vec<Cron>>
    /// Returns the new cron's id.
    15 CreateCron { label: String, prompt: String, schedule: String } => Call<String>
    /// Returns whether the cron is now enabled.
    16 ToggleCron { id: String } => Call<bool>
    17 DeleteCron { id: String } => Call<()>
    18 UpdateCron { id: String, enabled: Option<bool>, label: Option<String>, schedule: Option<String> } => Call<()>
    19 RegisterDevice { device_id: String, fcm_token: String } => Call<()>
    20 UnregisterDevice { device_id: String } => Call<()>
    /// `place` is the phone's reverse-geocoded name for the fix, such as
    /// "Shinjuku, Tokyo, Japan", when it has one.
    21 UpdateLocation { latitude: f64, longitude: f64, accuracy: f64, timezone: String, place: Option<String> } => Call<()>
    23 StreamNotifications {} => Stream<Notice>
    /// The curated apps the assistant can use, with this user's connection state.
    28 ListApps {} => Call<Vec<AppIntegration>>
    /// Starts OAuth for an app; open the returned URL in a browser.
    29 ConnectApp { slug: String } => Call<ConnectLink>
    30 DisconnectApp { slug: String } => Call<()>
    /// Personalized conversation starters, cached per user for 12 hours.
    /// `refresh` skips the cache, at most 3 times in 10 minutes.
    31 SuggestPrompts { refresh: bool } => Call<PromptSuggestions>
    /// Starts OAuth for an MCP app such as Fastmail, whose provider only
    /// redirects to localhost. The phone listens on `redirect_uri`, an
    /// `http://localhost:<port>/...` address, and opens the returned URL.
    32 StartAppAuth { slug: String, redirect_uri: String } => Call<ConnectLink>
    /// Hands the redirect's `code`, `state` and `iss` to the server, which
    /// keeps the tokens and refreshes them.
    33 FinishAppAuth { slug: String, code: String, state: String, issuer: Option<String> } => Call<()>
    /// Replaces a memory's text; it then counts as said by the user.
    34 UpdateMemory { id: String, content: String } => Call<()>
    /// Returns how many memories were deleted.
    35 ClearMemories {} => Call<u64>
    /// Returns how many of the sessions existed and were deleted.
    36 DeleteSessions { ids: Vec<String> } => Call<u64>
    /// Runs a cron now, keeping its schedule; a one-off reminder is used up.
    /// Returns the session the run went to.
    37 RunCron { id: String } => Call<String>
    /// What Beep may do in a connected app, and which of its actions act.
    38 GetAppPermissions { slug: String } => Call<AppPermissions>
    /// `allowed` names the actions, by tool, that run without asking under
    /// `Ask`.
    39 SetAppPermissions { slug: String, level: PermissionLevel, allowed: Vec<String> } => Call<()>
    /// Answers a tool call the session stopped on to ask the user, and
    /// resumes the session in a new run. Resending the same answer returns
    /// the same run.
    40 AnswerApproval { session_id: Uuid, query_id: String, decision: ApprovalDecision } => Call<Run>
    /// The models an admin can pick for their own turns. Empty for everyone else.
    41 GetModels {} => Call<Models>
    /// Picks the model for the caller's turns. Admins only.
    42 SetModel { id: String } => Call<Models>
    /// The ones of `ids` the server still has, so a phone can drop the
    /// chats another phone deleted.
    43 ExistingSessions { ids: Vec<String> } => Call<Vec<String>>
    /// Removes the turn whose user message has `timestamp`, as `HistoryEntry`
    /// gives it, with the reply and tool calls that followed it, from both
    /// the history and what the model sees. Fails while the chat has a run.
    44 DeleteTurn { session_id: String, timestamp: i64 } => Call<()>
    /// Picks how long the model reasons on the caller's turns. Admins only.
    45 SetThinking { level: ReasoningLevel } => Call<Models>
    /// Like `DeleteTurn`, and also removes every turn after it, so the chat
    /// ends before that prompt. A rerun sends the prompt again after this.
    46 DeleteTurnsFrom { session_id: String, timestamp: i64 } => Call<()>
    /// The sessions where the user or Beep wrote `query`, ignoring case,
    /// most recently changed first.
    47 SearchSessions { query: String } => Call<Vec<SessionMatch>>
    /// Turns Auto on or off for the caller: Jev then picks the model and
    /// thinking of each prompt from the server's Auto models. Admins only.
    48 SetAutoModel { enabled: bool } => Call<Models>
    /// Lets the device that made `offer` sign into the caller's account.
    /// Only the account's phone may link, and only a key with no account.
    49 LinkDevice { offer: LinkOffer } => Call<LinkedDevice>
    /// The account's keys: the phone first, then linked devices.
    50 ListDevices {} => Call<Vec<LinkedDevice>>
    /// Unlinks a device. The phone may unlink any; a device only itself.
    51 RemoveDevice { id: String } => Call<()>
    /// Freezes a copy of the session that anyone with the link can open,
    /// replacing the copy an earlier share made and keeping its link. The
    /// copy holds the messages only: no tool results, reasoning or context.
    52 ShareSession { id: String } => Call<SharedLink>
    /// Deletes the session's shared copy, so its link stops working.
    53 UnshareSession { id: String } => Call<()>
    /// The shared copy behind a link's `token`, made by any account.
    54 GetShare { token: String } => Call<SharedSession>
    /// Copies a shared session into the caller's chats as `session_id`, so
    /// they can carry it on. Returns the new session.
    55 CopyShare { token: String, session_id: Uuid } => Call<Session>
    /// The phone's timezone, such as "Europe/Madrid", which routines and
    /// reminders run in. Sent on connect, so no location is needed for it.
    56 SetTimezone { timezone: String } => Call<()>
}

/// The link prefix a shared session's token follows.
pub const SHARE_URL_PREFIX: &str = "beep://share/";

/// The link to the shared session `token`.
pub fn share_url(token: &str) -> String {
    format!("{SHARE_URL_PREFIX}{token}")
}

/// The token in a shared session's link, if `url` is one.
pub fn share_token(url: &str) -> Option<&str> {
    let token = url.trim().strip_prefix(SHARE_URL_PREFIX)?;
    (!token.is_empty() && token.bytes().all(|b| b.is_ascii_alphanumeric())).then_some(token)
}

wire_enum! {
    /// Why the server refused or failed a request.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum ApiError {
        0 BadRequest { message: String }
        1 NotFound {}
        2 Conflict { message: String }
        3 Internal {}
        4 Unauthorized {}
    }
}

impl ApiError {
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::BadRequest {
            message: message.into(),
        }
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadRequest { message } => write!(f, "bad request: {message}"),
            Self::NotFound {} => f.write_str("not found"),
            Self::Conflict { message } => write!(f, "conflict: {message}"),
            Self::Internal {} => f.write_str("internal error"),
            Self::Unauthorized {} => f.write_str("unauthorized"),
        }
    }
}

impl std::error::Error for ApiError {}

/// Server-side failures are logged here and reach the client only as `Internal`.
impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        log::warn!("request failed: {e:#}");
        Self::Internal {}
    }
}

wire_enum! {
    /// One entry of a run's event stream. `RunFinished` carries the session's
    /// `updated_at` once the run saved its turn, `None` when it saved none:
    /// the phone that streamed the run already holds that version.
    #[derive(Debug, Clone)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum RunEvent {
        0 TextDelta { delta: String }
        1 ToolStarted { name: String, call_id: String, description: String }
        2 ToolResult { name: String, call_id: String, output: String, success: bool }
        3 Usage { usage: Usage }
        4 Error { message: String }
        5 DeviceQuery { query_id: String, query: DeviceQuery }
        6 RunFinished { status: String, updated_at: Option<SystemTime> }
        7 RunStarted { run_id: String, session_id: String }
        8 ReasoningDelta { delta: String }
        9 MemoryScore { probability: f64 }
        10 AutoPicked { pick: AutoPick }
    }
}

wire_enum! {
    /// Work the server asks the phone to do during a run. `Approve` asks the user to
    /// allow a tool call, answered as `{"decision": "allow"}`, where the
    /// decision is `allow`, `always` or `deny`.
    #[derive(Debug, Clone, PartialEq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum DeviceQuery {
        0 Contacts { name: String }
        1 Browser { command: BrowserCommand }
        3 Approve { approval: Approval }
    }
}

/// A tool call that waits for the user's OK, as the phone shows it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Approval {
    /// The app's name, such as "Fastmail".
    pub app: String,
    /// What the tool does, such as "send email".
    pub action: String,
    /// The call's arguments, labelled for reading.
    pub fields: Vec<ApprovalField>,
}

#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApprovalField {
    pub label: String,
    pub value: String,
}

/// An approval a run is waiting on, carried by a `Notice` so the phone can
/// answer it from the notification.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingApproval {
    pub run_id: String,
    pub query_id: String,
    pub approval: Approval,
}

wire_enum! {
    /// The user's answer to an `Approval`. `Always` also stops asking for
    /// that action.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum ApprovalDecision {
        0 Deny {}
        1 Allow {}
        2 Always {}
    }
}

wire_enum! {
    /// How freely Beep acts in an app. Reading is always allowed; the level
    /// covers tools that send, change or delete.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum PermissionLevel {
        0 Ask {}
        1 Allow {}
        2 ReadOnly {}
    }
}

/// How long the model reasons before it answers. `Off` sends no reasoning
/// setting, so the model uses its own default.
#[cfg(feature = "uniffi")]
#[uniffi::remote(Enum)]
pub enum ReasoningLevel {
    Off,
    Low,
    Medium,
    High,
}

impl ApprovalDecision {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Deny {} => "deny",
            Self::Allow {} => "allow",
            Self::Always {} => "always",
        }
    }
}

impl PermissionLevel {
    /// The name stored on the server.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ask {} => "ask",
            Self::Allow {} => "allow",
            Self::ReadOnly {} => "read",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "ask" => Some(Self::Ask {}),
            "allow" => Some(Self::Allow {}),
            "read" => Some(Self::ReadOnly {}),
            _ => None,
        }
    }
}

/// The models an admin can pick from and the one their turns use.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Models {
    /// Empty when the caller is not an admin.
    pub choices: Vec<ModelChoice>,
    /// The `id` of the model the caller's turns use.
    pub selected: String,
    /// How long the model reasons on the caller's turns. Defaults so a
    /// phone's cached `Models` from before this field still decodes.
    #[serde(default)]
    pub thinking: ReasoningLevel,
    /// Jev picks the model and thinking of each of the caller's prompts.
    #[serde(default)]
    pub auto: bool,
    /// The server has models for Auto to pick from.
    #[serde(default)]
    pub auto_available: bool,
}

/// The model and thinking Jev picked for a prompt.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutoPick {
    /// A `ModelChoice::id`.
    pub model: String,
    pub thinking: ReasoningLevel,
}

#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelChoice {
    /// The model as the server names it, for example `openrouter:openai/gpt-6-luna`.
    pub id: String,
    pub name: String,
    /// USD per million input tokens, when the provider lists it.
    pub input_price: Option<f64>,
    /// USD per million output tokens, when the provider lists it.
    pub output_price: Option<f64>,
    pub sees_images: bool,
    /// The server's default model.
    pub default: bool,
}

/// One app's permissions.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppPermissions {
    pub level: PermissionLevel,
    /// The app's tools that act, in the order Beep lists them.
    pub actions: Vec<AppAction>,
}

#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppAction {
    pub tool: String,
    /// Such as "Send email".
    pub label: String,
    /// Runs without asking under `Ask`.
    pub allowed: bool,
}

wire_enum! {
    /// An action for the in-app browser on the phone.
    #[derive(Debug, Clone, PartialEq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum BrowserCommand {
        0 Navigate { url: String }
        1 Click { selector: String }
        2 Type { selector: String, text: String }
        3 Scroll { direction: ScrollDirection }
        4 GetText { selector: String }
        5 GetPageInfo {}
        6 Back {}
        7 Forward {}
        8 Reload {}
        9 ReadPage {}
        10 ClickElement { index: u32 }
        11 TypeElement { index: u32, text: String, submit: bool }
        12 WaitFor { selector: String }
    }
}

wire_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum ScrollDirection {
        0 Up {}
        1 Down {}
    }
}

wire_enum! {
    /// The phone's answer to a `DeviceQuery`.
    #[derive(Debug, Clone, PartialEq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum DeviceAnswer {
        0 Ok { output: String }
        1 Err { message: String }
    }
}

wire_enum! {
    /// A session message. An `Assistant` entry's `tools` are the tool results
    /// of the turn it ends; its `content` is empty when the turn had no text.
    /// `memory_score` is Jev's score for the turn, on its last assistant entry,
    /// `reasoning` is the model's reasoning over the whole turn, and `model`
    /// the model that answered it, empty when not recorded.
    /// A `User` entry's `auto` is what Jev picked for it, when it did.
    /// `Scheduled` stands in for the prompt a routine woke the agent with;
    /// `label` is the routine's name, empty when it has none.
    /// `Approval` ends a session that stopped to ask before a tool call, so
    /// the phone can show the card again; `query_id` is the call to answer.
    #[derive(Debug, Clone, PartialEq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum HistoryEntry {
        0 User { content: String, timestamp: i64, auto: Option<AutoPick> }
        1 Assistant { content: String, timestamp: i64, tools: Vec<HistoryTool>, memory_score: Option<f64>, reasoning: String, model: String }
        2 Scheduled { label: String, timestamp: i64 }
        3 Approval { query_id: String, approval: Approval }
    }
}

/// A tool result kept in session history, so the client can rebuild its card.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryTool {
    pub name: String,
    pub call_id: String,
    /// The tool output, cut to at most 16 KiB.
    pub output: String,
    pub success: bool,
}

wire_enum! {
    /// Who provides an app connection: `Composio` apps are connected per user
    /// through OAuth, `Fastmail` is set up on the server with an API token.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum AppKind {
        0 Composio {}
        1 Fastmail {}
    }
}

/// An app the assistant can use on the user's behalf.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppIntegration {
    pub slug: String,
    pub name: String,
    /// One short line.
    pub description: String,
    pub logo_url: Option<String>,
    pub connected: bool,
    /// False when the server lacks the credentials this app needs, so it
    /// cannot be connected from the phone.
    pub configured: bool,
    pub kind: AppKind,
}

#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConnectLink {
    pub url: String,
}

/// A conversation starter: `title` goes on a chip, `prompt` is sent when tapped.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptSuggestion {
    pub title: String,
    pub prompt: String,
}

/// Starters for the empty chat. `personal` is false for the fixed fallback
/// sent when the model failed, which is not worth keeping.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptSuggestions {
    pub suggestions: Vec<PromptSuggestion>,
    pub personal: bool,
}

/// Tokens spent on one model call.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    pub cache_read_tokens: u32,
    pub cache_creation_tokens: u32,
}

/// One entry of a run's event stream, numbered from 1.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub seq: i64,
    pub event: RunEvent,
}

#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Run {
    pub id: String,
    pub session_id: String,
    pub status: String,
}

#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub pinned: bool,
    /// When its turns last changed, in milliseconds since the Unix epoch. A
    /// phone holding a copy from then has nothing to fetch.
    pub updated_at: i64,
}

/// A shared copy of a session.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SharedLink {
    pub token: String,
    pub url: String,
    pub title: String,
    /// When the copy was made, in milliseconds since the Unix epoch.
    pub shared_at: i64,
}

/// What a shared link shows: the session's messages as they were shared.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SharedSession {
    pub title: String,
    /// In milliseconds since the Unix epoch.
    pub shared_at: i64,
    pub entries: Vec<HistoryEntry>,
}

/// A session whose messages hold the searched words.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionMatch {
    pub session: Session,
    /// The words around the latest match, cut short with `...`.
    pub snippet: String,
    /// Whether the user wrote the matching message, rather than Beep.
    pub from_user: bool,
}

#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Memory {
    pub id: String,
    pub content: String,
    pub category: String,
    pub source: MemorySource,
    /// `YYYY-MM-DD` of the last change.
    pub updated: String,
    /// `YYYY-MM-DD` bounds of a fact that only holds for a while, like a trip.
    pub valid_from: Option<String>,
    pub valid_until: Option<String>,
    /// The words in the chat, email or Slack message the memory came from.
    #[serde(default)]
    pub quote: Option<String>,
}

wire_enum! {
    /// Who a memory came from: the user saying so, Beep picking it up from a
    /// chat, or Beep reading it in the user's email or Slack.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum MemorySource {
        0 User {}
        1 Learned {}
        2 Email {}
        3 Slack {}
    }
}

impl MemorySource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User {} => "user",
            Self::Learned {} => "learned",
            Self::Email {} => "email",
            Self::Slack {} => "slack",
        }
    }

    /// Reads `as_str`; anything else, including the old `auto`, is learned.
    pub fn parse(text: &str) -> Self {
        match text {
            "user" => Self::User {},
            "email" => Self::Email {},
            "slack" => Self::Slack {},
            _ => Self::Learned {},
        }
    }
}

#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cron {
    pub id: String,
    pub label: String,
    pub prompt: String,
    pub schedule: String,
    pub enabled: bool,
    pub created_at: String,
    pub run_once: bool,
    pub last_run_at: Option<String>,
    pub next_run_at: Option<String>,
}

wire_enum! {
    /// What kind of client a key belongs to. An account's own key is its
    /// phone; linked keys are computers.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum DeviceKind {
        0 Phone {}
        1 Desktop {}
    }
}

impl Default for DeviceKind {
    fn default() -> Self {
        Self::Phone {}
    }
}

impl DeviceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Phone {} => "phone",
            Self::Desktop {} => "desktop",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "phone" => Some(Self::Phone {}),
            "desktop" => Some(Self::Desktop {}),
            _ => None,
        }
    }
}

/// A key that signs into the account.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinkedDevice {
    /// The device's endpoint id.
    pub id: String,
    pub name: String,
    pub kind: DeviceKind,
    /// The account's own key, which cannot be unlinked.
    pub primary: bool,
    /// The key of the caller.
    pub this_device: bool,
    pub linked_at: SystemTime,
}

/// A message the server pushes to the user's open notification streams.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notice {
    pub title: String,
    pub body: String,
    pub session_id: String,
    /// Slug of an app that lost its login; tapping the notice opens Apps.
    pub app: Option<String>,
    /// A tool call waiting for the user's OK, answered from the notice.
    pub approval: Option<PendingApproval>,
    /// Nothing to show: `session_id` changed on the server, or the chat list
    /// did when it is empty, so the phone refreshes its copy.
    pub changed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trip_is_compact() {
        let request = GetRun { id: "r1".into() };
        let buf = rmp_serde::to_vec(&request).unwrap();
        // fixarray(1), fixstr(2) "r1"
        assert_eq!(buf, [0x91, 0xa2, b'r', b'1']);
        assert_eq!(rmp_serde::from_slice::<GetRun>(&buf).unwrap(), request);
        assert_eq!(rmp_serde::to_vec(&Health {}).unwrap(), [0x90]);

        let chat = Chat {
            session_id: Uuid::new_v4(),
            request_id: Uuid::new_v4(),
            message: "hi".into(),
            images: vec!["data:image/jpeg;base64,AAAA".into()],
            model: Some("openai/gpt-test".into()),
            thinking: Some(ReasoningLevel::High),
        };
        let buf = rmp_serde::to_vec(&chat).unwrap();
        assert_eq!(rmp_serde::from_slice::<Chat>(&buf).unwrap(), chat);
    }

    #[test]
    fn nested_types_round_trip() {
        let event = Event {
            seq: 7,
            event: RunEvent::DeviceQuery {
                query_id: "q".into(),
                query: DeviceQuery::Contacts { name: "Ana".into() },
            },
        };
        let buf = rmp_serde::to_vec(&event).unwrap();
        let decoded: Event = rmp_serde::from_slice(&buf).unwrap();
        assert_eq!(rmp_serde::to_vec(&decoded).unwrap(), buf);

        let query = DeviceQuery::Browser {
            command: BrowserCommand::TypeElement {
                index: 3,
                text: "rust".into(),
                submit: true,
            },
        };
        let buf = rmp_serde::to_vec(&query).unwrap();
        assert_eq!(rmp_serde::from_slice::<DeviceQuery>(&buf).unwrap(), query);
    }

    #[test]
    fn share_token_reads_only_share_links() {
        assert_eq!(share_token(&share_url("ab12")), Some("ab12"));
        assert_eq!(share_token(" beep://share/ab12\n"), Some("ab12"));
        assert_eq!(share_token("beep://share/"), None);
        assert_eq!(share_token("beep://share/../x"), None);
        assert_eq!(share_token("beep://apps"), None);
    }

    #[test]
    fn decode_rejects_bad_requests() {
        assert!(Op::try_from(0x7f).is_err(), "unknown op");
        assert!(
            rmp_serde::from_slice::<GetRun>(&[0x90]).is_err(),
            "missing argument"
        );
    }
}
