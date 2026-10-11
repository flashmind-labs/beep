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
pub mod search;
pub mod version;
pub mod wire;

#[cfg(test)]
#[macro_use]
mod compat;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

pub use flashmind_types::ReasoningLevel;
use link::LinkOffer;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{fmt, time::SystemTime};
use uuid::Uuid;
pub use version::{ClientInfo, Feature, WIRE};

// Kotlin and Swift see ids as their text.
#[cfg(feature = "uniffi")]
uniffi::custom_type!(Uuid, String, {
    remote,
    try_lift: |text| Ok(Uuid::parse_str(&text)?),
    lower: |id| id.to_string(),
});

macro_rules! one {
    ($x:tt) => {
        1
    };
}

/// Enum serialized as `[op, field...]` instead of serde's `{"Variant": ...}`.
/// A variant only newer clients read ends in `since X`, naming `Feature::X`,
/// and the server sends it only where `ClientInfo::supports` says so.
macro_rules! wire_enum {
    ($(#[$meta:meta])* pub enum $enum:ident {
        $($op:literal $name:ident { $($field:ident: $ty:ty),* $(,)? } $(since $since:ident)?)*
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

        #[cfg(test)]
        impl compat::Sample for $enum {
            fn samples(wire: u32) -> Vec<Self> {
                let mut out = Vec::new();
                $(
                    if true $(&& wire >= Feature::$since.wire())? {
                        sample_each!(out, wire, [Self::$name] { $($field),* });
                    }
                )*
                out
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
    ($($(#[$meta:meta])* $op:literal $name:ident { $($(#[$fmeta:meta])* $field:ident: $ty:ty),* $(,)? } => $kind:ident<$reply:ty>)*) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[repr(u8)]
        pub enum Op {
            $($name = $op,)*
        }

        impl Op {
            pub const ALL: &[Op] = &[$(Op::$name),*];
        }

        /// Hands `visit` every request type with its reply type, in op order.
        #[cfg(test)]
        fn each_request(visit: &mut impl compat::Visit) {
            $(visit.request::<$name, $reply>();)*
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
                $($(#[$fmeta])* pub $field: $ty,)*
            }

            impl Rpc for $name {
                const OP: Op = Op::$name;
            }

            impl $kind for $name {
                type Reply = $reply;
            }

            #[cfg(test)]
            sample_struct!($name { $($field),* });
        )*
    };
}

requests! {
    /// Opens every connection: the server reads it before any other stream,
    /// and sends nothing newer than `wire` on the connection. Clients from
    /// before it never send it and read wire 1. A `Hello` later in a
    /// connection changes nothing.
    0 Hello { wire: u32, platform: Option<Platform>, version: Option<String> } => Call<()>
    1 Health {} => Call<()>
    /// Starts a run and streams its events, starting with `RunStarted`. Resending
    /// the same `request_id` follows the existing run instead of starting another.
    /// `images` are `data:image/...;base64,` URIs; the server keeps them with
    /// the chat, and `GetSessionImages` lists them. `model` is a `ModelChoice::id` to run this
    /// one turn on instead of the picked model, and `thinking` how long it
    /// reasons; only admins may set either. `platform` is the app sending it,
    /// missing from apps older than the field.
    2 Chat {
        session_id: Uuid,
        request_id: Uuid,
        message: String,
        images: Vec<String>,
        model: Option<String>,
        thinking: Option<ReasoningLevel>,
        #[serde(default)]
        platform: Option<Platform>,
    } => Stream<Event>
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
    /// Returns the new cron's id. A `schedule` of `arrive:<place id>` or
    /// `leave:<place id>` starts it on arriving at or leaving a saved place
    /// instead of at a time; `arrive:<place id>:daily` runs once a day at most.
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
    /// Makes a group chat `id` with the caller as its first member, shown
    /// to the others as `member_name`.
    57 CreateGroup { id: Uuid, name: String, member_name: String } => Call<Group>
    /// The caller's groups, most recently active first.
    58 ListGroups {} => Call<Vec<Group>>
    /// A group with its latest messages, oldest first.
    59 GetGroup { id: String } => Call<GroupChat>
    /// Posts to a group. A message that mentions `@beep` asks Beep to
    /// reply, running as the sender. Sending the same `request_id` twice
    /// posts once. `images` are `data:image/...;base64,` URIs, kept with the
    /// group and listed by `GetGroupImages`; older clients send none.
    60 SendToGroup { group_id: String, request_id: Uuid, message: String, #[serde(default)] images: Vec<String> } => Call<GroupMessage>
    /// The group's invite link. `reset` makes a new one, so the old link
    /// stops working.
    61 GroupInvite { id: String, reset: bool } => Call<GroupLink>
    /// What an invite link's `token` leads to, before joining.
    62 PeekGroup { token: String } => Call<GroupPreview>
    /// Joins the group behind an invite `token` as `member_name`.
    63 JoinGroup { token: String, member_name: String } => Call<Group>
    /// Leaves a group. The last member to leave deletes it.
    64 LeaveGroup { id: String } => Call<()>
    /// Pins a group to the top of the caller's list, as `PinSession` does a chat.
    65 PinGroup { id: String, pinned: bool } => Call<()>
    /// Lets Beep use the caller's apps in a group when other members ask, or
    /// stops it. Their own asks always may.
    66 ShareGroupApps { id: String, shared: bool } => Call<()>
    /// Answers an app action Beep asked to take for the caller from a group,
    /// sent as a `Notice` with `group` set. Allowing runs it, and Beep tells
    /// the group how it went.
    67 AnswerGroupApproval { query_id: String, allow: bool } => Call<()>
    /// Up to `FILE_PART` bytes of a file Beep sent, from `offset` on, as a
    /// `send_file` tool result names it by `id`.
    68 GetFile { id: String, offset: u64 } => Call<FilePart>
    /// The reminders set in a group, soonest first. Members only.
    70 ListGroupReminders { group_id: String } => Call<Vec<GroupReminder>>
    /// Deletes one of a group's reminders. Any member may.
    71 DeleteGroupReminder { group_id: String, id: String } => Call<()>
    /// The pictures the user sent in a session, by the `timestamp` of the
    /// `HistoryEntry::User` they came with. `GetFile` reads each one.
    72 GetSessionImages { session_id: String } => Call<Vec<SentImages>>
    /// The pictures members sent in a group, by the `seq` of their
    /// `GroupMessage`. Members only.
    73 GetGroupImages { group_id: String } => Call<Vec<SentImages>>
    /// The caller's saved places, by name.
    74 ListPlaces {} => Call<Vec<Place>>
    /// Forgets a place, and the routines that start there with it.
    75 DeletePlace { id: String } => Call<()>
    /// Renames a place or changes how near counts as being there.
    /// `radius` is in metres, 25 to 1000.
    76 UpdatePlace { id: String, name: Option<String>, radius: Option<u32> } => Call<Place>
    /// The phone's own geofencing saw it arrive at or leave saved place
    /// `id`, so the server starts that place's routines without waiting for
    /// a location fix of its own. Returns whether it changed where the server
    /// had the phone; false when it already knew, so nothing started.
    77 CrossPlace { id: String, crossing: PlaceCrossing } => Call<bool>
    /// The reads of the caller's mail and calendars that added or confirmed
    /// memories, newest first. A notice with no `session_id`, `app` or
    /// `approval` is one finishing, so tapping it opens the newest here.
    78 ListMemoryReads {} => Call<Vec<MemoryRead>>
    /// The caller's updates after `after`, the last update id the client
    /// holds, oldest first. A client starts from 0 and calls again with
    /// `UpdatePage::last` while `more` is set.
    79 GetUpdates { after: i64 } => Call<UpdatePage>
    /// The caller's newest update id now and whenever it changes, so a
    /// client knows when to call `GetUpdates`.
    80 StreamUpdates {} => Stream<i64>
    /// The caller's skills and those of every group they are in, the
    /// caller's own first, each most recently used first.
    81 ListSkills {} => Call<Vec<Skill>>
    /// Changes what is given; the old version stays for `UndoSkill`. A
    /// group's skill may be changed by any member.
    82 UpdateSkill {
        id: String,
        name: Option<String>,
        description: Option<String>,
        body: Option<String>,
        enabled: Option<bool>,
    } => Call<Skill>
    83 DeleteSkill { id: String } => Call<()>
    /// Puts back the version before the skill's last change.
    84 UndoSkill { id: String } => Call<Skill>
}

/// A picture someone sent, kept on the server as long as its chat. Read its
/// bytes with `GetFile`.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageRef {
    pub id: String,
    pub mime: String,
    pub width: u32,
    pub height: u32,
}

/// The pictures one message carried, in the order they were sent.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SentImages {
    /// The `HistoryEntry::User` `timestamp` in a session, the `GroupMessage`
    /// `seq` in a group.
    pub message: i64,
    pub images: Vec<ImageRef>,
}

/// The most of a file one `GetFile` returns, well under a frame.
pub const FILE_PART: u64 = 4 * 1024 * 1024;

/// Part of a sent file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilePart {
    #[serde(with = "serde_bytes")]
    pub data: Vec<u8>,
    /// The whole file's size, so the caller knows when it has it all.
    pub size: u64,
}

/// The web host share and invite links point at. The Cloudflare Worker in
/// `deploy/links` serves it, and the Android manifest claims it for App Links.
pub const LINK_HOST: &str = "beep.flashmind.com";

/// The link to the shared session `token`.
pub fn share_url(token: &str) -> String {
    format!("https://{LINK_HOST}/s/{token}")
}

/// The token in a shared session's link, if `url` is one. Old `beep://share/`
/// links still read.
pub fn share_token(url: &str) -> Option<&str> {
    link_token(url, "s", "beep://share/")
}

/// The invite link with `token`.
pub fn group_url(token: &str) -> String {
    format!("https://{LINK_HOST}/g/{token}")
}

/// The token in a group invite link, if `url` is one. Old `beep://group/`
/// links still read.
pub fn group_token(url: &str) -> Option<&str> {
    link_token(url, "g", "beep://group/")
}

/// The token after `https://LINK_HOST/<path>/` or the older `legacy` prefix.
fn link_token<'a>(url: &'a str, path: &str, legacy: &str) -> Option<&'a str> {
    let url = url.trim();
    let token = url.strip_prefix(legacy).or_else(|| {
        url.strip_prefix("https://")?
            .strip_prefix(LINK_HOST)?
            .strip_prefix('/')?
            .strip_prefix(path)?
            .strip_prefix('/')
    })?;
    let token = token.strip_suffix('/').unwrap_or(token);
    (!token.is_empty() && token.bytes().all(|b| b.is_ascii_alphanumeric())).then_some(token)
}

/// The invite code in what someone typed or pasted: a code with any case,
/// spaces or dashes, or an invite link. None for other text.
pub fn group_code(input: &str) -> Option<String> {
    let input = group_token(input).unwrap_or(input.trim());
    let code: String = input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .map(|c| c.to_ascii_uppercase())
        .collect();
    let valid = (4..=64).contains(&code.len()) && code.bytes().all(|b| b.is_ascii_alphanumeric());
    valid.then_some(code)
}

/// `code` as people read it: `K7QM4-X9TRP` for a ten-character code.
pub fn format_group_code(code: &str) -> String {
    match code.len() {
        10 if code.is_ascii() => format!("{}-{}", &code[..5], &code[5..]),
        _ => code.to_owned(),
    }
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

/// One read of a mailbox or calendar, and the memories it added or
/// confirmed. A memory a later read confirms moves to that read.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryRead {
    pub id: String,
    /// The app that was read, such as "Gmail" or "Outlook calendar".
    pub source: String,
    /// RFC 3339 of when the read finished.
    pub finished_at: String,
    /// The `Memory` ids, in the order `ListMemories` gives them.
    pub memories: Vec<String>,
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

/// A reminder Beep posts into a group when it is due.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupReminder {
    pub id: String,
    pub label: String,
    /// What Beep posts.
    pub message: String,
    /// RFC 3339 for a one-off, else a cron expression in `set_by`'s timezone.
    pub schedule: String,
    pub next_run_at: Option<String>,
    /// The member who asked for it, by their name in the group.
    pub set_by: String,
}

/// A way of doing something that Beep learned from how the user, or a
/// group, asks for it. Beep writes and updates them on its own.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Skill {
    pub id: String,
    pub name: String,
    /// One line on when it applies.
    pub description: String,
    /// The instructions, in Markdown.
    pub body: String,
    /// The group it belongs to; both `None` for the user's own.
    pub group_id: Option<String>,
    pub group_name: Option<String>,
    /// A paused skill stays, but Beep does not use it.
    pub enabled: bool,
    /// How many times Beep used it.
    pub uses: u32,
    /// RFC 3339.
    pub last_used: Option<String>,
    /// RFC 3339 of the last change.
    pub updated_at: String,
    /// Whether the user changed it last, rather than Beep.
    pub edited: bool,
    /// Whether an earlier version is kept for `UndoSkill`.
    pub can_undo: bool,
}

wire_enum! {
    /// Arriving at a saved place or leaving it.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum PlaceCrossing {
        0 Arrive {}
        1 Leave {}
    }
}

/// A place the user saved by name, such as Home or Gym, that routines can
/// start on arriving at or leaving.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Place {
    pub id: String,
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    /// Within this many metres counts as being there.
    pub radius: u32,
    /// A street address or area name, when known.
    pub address: Option<String>,
    pub created_at: String,
    /// Whether the phone's last location was there.
    pub here: bool,
    /// When the phone was last there.
    pub last_here_at: Option<String>,
    /// How many routines start on arriving at or leaving it.
    pub routines: u32,
}

#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cron {
    pub id: String,
    pub label: String,
    pub prompt: String,
    /// RFC 3339 for a one-off, a place trigger as `CreateCron` describes,
    /// else a cron expression.
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

wire_enum! {
    /// Which app a request comes from.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum Platform {
        0 Android {}
        1 Ios {}
        2 Macos {}
    }
}

impl Platform {
    /// How the model is told where the user is writing from.
    pub fn describe(self) -> &'static str {
        match self {
            Self::Android {} => "the Android app on their phone",
            Self::Ios {} => "the iOS app on their iPhone",
            Self::Macos {} => "the macOS app on their computer",
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

/// A group chat as its list row shows it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub id: String,
    pub name: String,
    /// Oldest first; Beep is not listed.
    pub members: Vec<GroupMember>,
    pub last: Option<GroupMessage>,
    /// When the last message came in, in milliseconds since the Unix epoch.
    pub updated_at: i64,
    /// Beep is writing a reply.
    pub beep_typing: bool,
    /// Pinned to the top of the caller's list.
    #[serde(default)]
    pub pinned: bool,
    /// Beep may use the caller's apps here when other members ask.
    #[serde(default = "shared_by_default")]
    pub apps_shared: bool,
}

fn shared_by_default() -> bool {
    true
}

#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupMember {
    pub name: String,
    /// The caller.
    pub you: bool,
    /// Made the group.
    pub creator: bool,
    /// In milliseconds since the Unix epoch.
    pub joined_at: i64,
}

/// A group with its latest messages, oldest first.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupChat {
    pub group: Group,
    pub messages: Vec<GroupMessage>,
}

/// One message in a group. Beep's replies hold text only: its thinking and
/// tool calls never reach the group.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupMessage {
    /// Grows with each message in the group.
    pub seq: i64,
    pub author: GroupAuthor,
    pub content: String,
    /// In milliseconds since the Unix epoch.
    pub timestamp: i64,
}

wire_enum! {
    /// Who wrote a group message. `you` marks the caller's own messages.
    #[derive(Debug, Clone, PartialEq, Eq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum GroupAuthor {
        0 Member { name: String, you: bool }
        1 Beep {}
    }
}

/// A group's invite link.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupLink {
    pub token: String,
    pub url: String,
}

/// What an invite shows before joining.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupPreview {
    pub id: String,
    pub name: String,
    /// Member names, oldest first.
    pub members: Vec<String>,
    /// The caller is in it already.
    pub joined: bool,
}

wire_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum ChatKind {
        0 Session {}
        1 Group {}
    }
}

/// A chat as `GetUpdates` sends it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatInfo {
    pub id: Uuid,
    pub kind: ChatKind,
    /// A session's title or a group's name.
    pub title: String,
    /// Pinned to the top of the caller's list.
    pub pinned: bool,
    /// A group's members, oldest first; empty for a session.
    pub members: Vec<GroupMember>,
    /// Beep may use the caller's apps in this group when others ask.
    pub apps_shared: bool,
}

/// One step of a chat: a message, a tool call, an approval card.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatMessage {
    /// Grows with each message on the server, across chats.
    pub id: i64,
    /// The message that opened its turn, its own id for that one. `None` in
    /// groups.
    pub turn_id: Option<i64>,
    /// In milliseconds since the Unix epoch.
    pub timestamp: i64,
    pub body: MessageBody,
}

/// What a turn ran with. It sits on the message that opened the turn and
/// changes as the turn goes on, which sends that message again.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TurnInfo {
    /// The model that answered, empty when not recorded.
    pub model: String,
    /// What Jev picked, when it did.
    pub auto: Option<AutoPick>,
    /// The model's reasoning over the whole turn.
    pub reasoning: String,
    pub memory_score: Option<f64>,
}

wire_enum! {
    /// `User` and `Scheduled` open a turn and carry it. `Member` and `Beep`
    /// are group messages; `you` marks the caller's own.
    #[derive(Debug, Clone, PartialEq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum MessageBody {
        0 User { content: String, images: Vec<ImageRef>, turn: TurnInfo }
        1 Scheduled { label: String, turn: TurnInfo }
        2 Assistant { content: String }
        3 Tool { call_id: String, name: String, output: String, success: bool }
        4 Approval { query_id: String, approval: Approval }
        5 Member { name: String, you: bool, content: String, images: Vec<ImageRef> }
        6 Beep { content: String }
    }
}

wire_enum! {
    /// One entry of the caller's log. A message that changes comes again
    /// under a newer id; `Deleted` and `ChatDeleted` tell the client to drop
    /// its copy.
    #[derive(Debug, Clone, PartialEq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum Update {
        0 Message { update_id: i64, chat_id: Uuid, message: ChatMessage }
        1 Deleted { update_id: i64, chat_id: Uuid, message_id: i64 }
        2 Chat { update_id: i64, chat: ChatInfo }
        3 ChatDeleted { update_id: i64, chat_id: Uuid }
    }
}

/// A page of the caller's updates.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdatePage {
    pub updates: Vec<Update>,
    /// The update id to send as `after` next.
    pub last: i64,
    /// More updates follow; call again.
    pub more: bool,
    /// The server no longer has every update after `after`: drop every
    /// chat and start over from `updates`.
    pub reset: bool,
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
    /// `session_id` is a group's id. Defaults so a phone from before groups
    /// still decodes notices.
    #[serde(default)]
    pub group: bool,
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
            platform: Some(Platform::Ios {}),
        };
        let buf = rmp_serde::to_vec(&chat).unwrap();
        assert_eq!(rmp_serde::from_slice::<Chat>(&buf).unwrap(), chat);
    }

    #[test]
    fn a_place_crossing_decodes() {
        let request = CrossPlace {
            id: "p1".into(),
            crossing: PlaceCrossing::Leave {},
        };
        let buf = rmp_serde::to_vec(&request).unwrap();
        // fixarray(2), fixstr(2) "p1", fixarray(1) op 1
        assert_eq!(buf, [0x92, 0xa2, b'p', b'1', 0x91, 0x01]);
        assert_eq!(rmp_serde::from_slice::<CrossPlace>(&buf).unwrap(), request);
        let unknown = [0x92, 0xa2, b'p', b'1', 0x91, 0x07];
        assert!(rmp_serde::from_slice::<CrossPlace>(&unknown).is_err());
    }

    #[test]
    fn chat_from_an_app_without_platform_decodes() {
        let old = (
            Uuid::nil(),
            Uuid::nil(),
            "hi",
            Vec::<String>::new(),
            None::<String>,
            None::<ReasoningLevel>,
        );
        let buf = rmp_serde::to_vec(&old).unwrap();
        assert_eq!(rmp_serde::from_slice::<Chat>(&buf).unwrap().platform, None);
    }

    #[test]
    fn a_group_message_from_before_images_still_decodes() {
        #[derive(Serialize)]
        struct Old {
            group_id: String,
            request_id: Uuid,
            message: String,
        }
        let old = Old {
            group_id: "g1".into(),
            request_id: Uuid::new_v4(),
            message: "hi".into(),
        };
        let buf = rmp_serde::to_vec(&old).unwrap();
        let decoded: SendToGroup = rmp_serde::from_slice(&buf).unwrap();
        assert_eq!(decoded.message, "hi");
        assert!(decoded.images.is_empty());
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
        assert_eq!(share_url("ab12"), "https://beep.flashmind.com/s/ab12");
        assert_eq!(share_token(&share_url("ab12")), Some("ab12"));
        assert_eq!(
            share_token(" https://beep.flashmind.com/s/ab12/\n"),
            Some("ab12")
        );
        assert_eq!(share_token(" beep://share/ab12\n"), Some("ab12"));
        assert_eq!(share_token("beep://share/"), None);
        assert_eq!(share_token("beep://share/../x"), None);
        assert_eq!(share_token("https://beep.flashmind.com/s/"), None);
        assert_eq!(share_token("https://beep.flashmind.com.evil/s/ab12"), None);
        assert_eq!(share_token("http://beep.flashmind.com/s/ab12"), None);
        assert_eq!(share_token("beep://apps"), None);
        assert_eq!(group_token(&group_url("cd34")), Some("cd34"));
        assert_eq!(group_token("beep://group/cd34"), Some("cd34"));
        assert_eq!(group_token(&share_url("cd34")), None);
    }

    #[test]
    fn group_code_reads_typed_codes_and_old_links() {
        assert_eq!(group_code("k7qm4-x9trp").as_deref(), Some("K7QM4X9TRP"));
        assert_eq!(group_code(" K7QM4 X9TRP\n").as_deref(), Some("K7QM4X9TRP"));
        assert_eq!(
            group_code(&group_url("7a2d5f90be13")).as_deref(),
            Some("7A2D5F90BE13")
        );
        assert_eq!(
            group_code("beep://group/7a2d5f90be13").as_deref(),
            Some("7A2D5F90BE13")
        );
        assert_eq!(group_code("hey there!"), None);
        assert_eq!(group_code("ab"), None);
        assert_eq!(group_code(&share_url("cd34ef")), None);
        assert_eq!(format_group_code("K7QM4X9TRP"), "K7QM4-X9TRP");
        assert_eq!(format_group_code("7A2D5F90BE13"), "7A2D5F90BE13");
    }

    impl Feature {
        const LATER: Feature = Feature(u32::MAX);
    }

    wire_enum! {
        #[derive(Debug, Clone, PartialEq)]
        pub enum Shape {
            0 Old { size: u32 }
            1 New {} since LATER
        }
    }

    #[test]
    fn a_gated_variant_is_left_out_of_older_samples() {
        use compat::Sample;
        assert_eq!(Shape::samples(1), [Shape::Old { size: 42 }]);
        assert_eq!(Shape::samples(u32::MAX).len(), 2);
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
