//! The Beep protocol: messages, framing and client, shared by the server and
//! the Android app.
//!
//! Each request is a struct declared in `requests!` with its op and reply
//! type. Enums made with `wire_enum!` encode as `[op, field...]`; structs
//! encode as their fields in declaration order.
//!
//! With the `uniffi` feature, the types a client sees also derive UniFFI
//! bindings.

pub mod agent_ids;
mod agent_notice;
pub mod client;
pub mod flsh;
pub mod groups;
pub mod link;
#[cfg(feature = "logging")]
pub mod logging;
pub mod names;
pub mod node;
pub mod seal;
pub mod search;
pub mod secret;
pub mod setup;
pub mod version;
pub mod wire;

#[cfg(test)]
#[macro_use]
mod compat;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

pub use agent_ids::{AgentSlug, ModelId};
pub use agent_notice::{AgentNoticeKey, NotAgentKey};
pub use flashmind_types::ReasoningLevel;
pub use flsh::{
    FlshAutoCompact, FlshModel, FlshModelRule, FlshQuantization, FlshSettings, FlshSettingsError,
    FlshSort,
};
use link::LinkOffer;
pub use secret::SecretText;
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

/// A `Stream` request that also sends: after its request frame, the
/// client sends any number of `Up` frames and finishes its side when done.
pub trait Duplex: Stream {
    type Up: Serialize + DeserializeOwned + Send + Sync + 'static;
}

/// Declares every request: its op, its fields and what it replies with.
macro_rules! requests {
    ($($(#[$meta:meta])* $op:literal $name:ident { $($(#[$fmeta:meta])* $field:ident: $ty:ty),* $(,)? } => $kind:ident<$reply:ty>)*) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[repr(u16)]
        pub enum Op {
            $($name = $op,)*
        }

        impl Op {
            pub const ALL: &[Op] = &[$(Op::$name),*];
        }

        // A request frame's tag 255 says a wide op follows, so no request
        // has that number.
        const _: () = assert!($($op != wire::WIDE as u16 &&)* true, "op 255 is reserved");

        /// Hands `visit` every request type with its reply type, in op order.
        #[cfg(test)]
        fn each_request(visit: &mut impl compat::Visit) {
            $(visit.request::<$name, $reply>();)*
        }

        impl TryFrom<u16> for Op {
            type Error = ApiError;

            fn try_from(op: u16) -> Result<Self, ApiError> {
                match op {
                    $($op => Ok(Op::$name),)*
                    _ => Err(ApiError::bad_request(format!("unknown op {op}"))),
                }
            }
        }

        $(
            $(#[$meta])*
            #[derive(Debug, Clone, PartialEq, Serialize)]
            pub struct $name {
                $($(#[$fmeta])* pub $field: $ty,)*
            }

            /// Reads the fields this build knows and skips any a newer app
            /// added after them, so a request only ever gains fields.
            impl<'de> Deserialize<'de> for $name {
                fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                    #[derive(Deserialize)]
                    #[serde(rename = "Request")]
                    struct Known {
                        $($(#[$fmeta])* $field: $ty,)*
                    }

                    struct Lenient;

                    impl<'de> serde::de::Visitor<'de> for Lenient {
                        type Value = Known;

                        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                            f.write_str("a request as an array")
                        }

                        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Known, A::Error> {
                            let known = Known::deserialize(serde::de::value::SeqAccessDeserializer::new(&mut seq))?;
                            while seq.next_element::<serde::de::IgnoredAny>()?.is_some() {}
                            Ok(known)
                        }
                    }

                    let Known { $($field),* } = d.deserialize_seq(Lenient)?;
                    Ok(Self { $($field),* })
                }
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
    /// reasons. `platform` is the app sending it,
    /// missing from apps older than the field. `files` are other files sent
    /// with the message, which the model reads and `GetSessionFiles` lists;
    /// older apps send none. `research` starts a research in a new chat:
    /// a lead agent plans it with the user, then a team of agents researches
    /// it in the background (see `GetResearch`). `voice` is a spoken message
    /// of at most `MAX_VOICE` bytes, of one of `VOICE_TYPES`: the server
    /// transcribes it, streams the transcript as `RunEvent::Transcript` and
    /// runs it as the message, and keeps the recording with the turn like
    /// `files`. `message` may then be empty. `spoken` marks a message said
    /// in live mode, so the apps can show those turns as a transcript (see
    /// `SpokenTurns`); a `voice` message is spoken either way.
    2 Chat {
        session_id: Uuid,
        request_id: Uuid,
        message: String,
        images: Vec<String>,
        model: Option<String>,
        thinking: Option<ReasoningLevel>,
        #[serde(default)]
        platform: Option<Platform>,
        #[serde(default)]
        files: Vec<Upload>,
        #[serde(default)]
        research: bool,
        #[serde(default)]
        voice: Option<Upload>,
        #[serde(default)]
        spoken: bool,
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
    /// instead of at a time; ending it in `:daily` runs it once a day at most,
    /// and in `:once` only the next time.
    15 CreateCron { label: String, prompt: String, schedule: String } => Call<String>
    /// Returns whether the cron is now enabled.
    16 ToggleCron { id: String } => Call<bool>
    17 DeleteCron { id: String } => Call<()>
    18 UpdateCron { id: String, enabled: Option<bool>, label: Option<String>, schedule: Option<String> } => Call<()>
    /// `fcm_token` is the push token: Firebase's, or with `apns` set, the
    /// one Apple gave an iPhone for that environment.
    19 RegisterDevice {
        device_id: String,
        fcm_token: String,
        #[serde(default)]
        apns: Option<ApnsEnvironment>,
    } => Call<()>
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
    /// the same run. `for_session` with `Allow` also lets the same action
    /// run without asking for the rest of this chat ("Allow for this
    /// session"); for a card of background work in the chat, such as a
    /// coding agent's question, its later cards of the same kind are
    /// allowed too. Servers from before it allow once.
    40 AnswerApproval { session_id: Uuid, query_id: String, decision: ApprovalDecision, #[serde(default)] for_session: bool } => Call<Run>
    /// The models the caller can pick for their own turns.
    41 GetModels {} => Call<Models>
    /// Picks the model for the caller's turns.
    42 SetModel { id: String } => Call<Models>
    /// The ones of `ids` the server still has, so a phone can drop the
    /// chats another phone deleted.
    43 ExistingSessions { ids: Vec<String> } => Call<Vec<String>>
    /// Removes the turn whose user message has `timestamp`, as `HistoryEntry`
    /// gives it, with the reply and tool calls that followed it, from both
    /// the history and what the model sees. Fails while the chat has a run.
    44 DeleteTurn { session_id: String, timestamp: i64 } => Call<()>
    /// Picks how long the model reasons on the caller's turns.
    45 SetThinking { level: ReasoningLevel } => Call<Models>
    /// Like `DeleteTurn`, and also removes every turn after it, so the chat
    /// ends before that prompt. A rerun sends the prompt again after this.
    46 DeleteTurnsFrom { session_id: String, timestamp: i64 } => Call<()>
    /// The sessions where the user or Beep wrote `query`, ignoring case,
    /// most recently changed first.
    47 SearchSessions { query: String } => Call<Vec<SessionMatch>>
    /// Turns Auto on or off for the caller: Jev then picks the model and
    /// thinking of each prompt from the server's Auto models.
    48 SetAutoModel { enabled: bool } => Call<Models>
    /// Lets the device that made `offer` sign into the caller's account.
    /// Any key of the account may link, and only a key with no account.
    49 LinkDevice { offer: LinkOffer } => Call<LinkedDevice>
    /// The account's keys: the primary key first, then linked devices.
    50 ListDevices {} => Call<Vec<LinkedDevice>>
    /// Unlinks a device. The primary key may unlink any; a device only
    /// itself.
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
    /// Whether agents are on, and the agents Jev picks from.
    85 GetAgents {} => Call<Agents>
    /// Turns agents on or off for every user. Ignored now: there are no
    /// agents to turn on.
    86 SetAgents { enabled: bool } => Call<Agents>
    /// The price chart of `symbol` over `range`, as `show_stock_chart`
    /// draws it, for a chart's range buttons.
    87 StockChart { symbol: String, range: ChartRange } => Call<Chart>
    /// A code another device scans or types to join the caller's account,
    /// bringing its own account's chats, memories and devices along.
    /// `name` is what the caller calls itself, such as "MacBook", and
    /// becomes its name in the device list. Any key of the account may invite.
    88 InviteDevice { name: String } => Call<DeviceInvite>
    /// Who made the invite `code`, before joining.
    89 PeekInvite { code: String } => Call<JoinPreview>
    /// Moves the caller's account into the one that made `code`: its chats,
    /// memories, routines, places and keys. Only an account's primary key
    /// may, and not while one of its chats has a run. The server then
    /// closes the moved keys' connections, so they sign in again.
    90 JoinAccount { code: String } => Call<()>
    /// The specialized agent each turn of a session ran as, by the
    /// `timestamp` of the `HistoryEntry::User` that started it. Every turn is
    /// listed, so the newest entry is the newest turn.
    91 GetSessionAgents { session_id: String } => Call<Vec<TurnAgent>>
    /// `GetRouteCard` as apps from before stop codes read it.
    92 GetRoute { call_id: String } => Call<RouteCardV1>
    /// The routes a `get_route` call found, by the call's id, for its card.
    /// `NotFound` for a call that found none or is not the caller's.
    93 GetRouteCard { call_id: String } => Call<RouteCard>
    /// The MCP servers the user added by URL.
    94 ListMcpServers {} => Call<Vec<McpServer>>
    /// Adds a remote MCP server by its https URL. One that needs no sign-in
    /// connects at once; otherwise open `auth_url`, which comes back through
    /// `mcp_callback_url()` with what `FinishMcpAuth` takes.
    95 AddMcpServer { name: String, url: String } => Call<McpConnect>
    /// Connects an added server again, signing in again when it needs to.
    96 ConnectMcpServer { id: String } => Call<McpConnect>
    /// Hands the sign-in redirect's `code`, `state` and `iss` to the server,
    /// which finds the server the flow was for by `state`.
    97 FinishMcpAuth { code: String, state: String, issuer: Option<String> } => Call<McpServer>
    98 RemoveMcpServer { id: String } => Call<()>
    /// The run going on in the caller's session, started from any of their
    /// devices, to follow with `GetEvents` from 0. `None` when it is idle.
    99 ActiveRun { session_id: Uuid } => Call<Option<Run>>
    /// The accounts of an app that connects several, such as Fastmail, in
    /// the order Beep uses them: the agent's default account comes first.
    /// Empty when none is connected.
    100 ListAppAccounts { slug: String } => Call<Vec<AppAccount>>
    /// Like `StartAppAuth`, but signs in another account instead of
    /// reconnecting the first. `FinishAppAuth` completes it. Signing in to an
    /// account Beep already has replaces its old login.
    101 AddAppAccount { slug: String, redirect_uri: String } => Call<ConnectLink>
    /// Disconnects one account, by `AppAccount::id`. `DisconnectApp` still
    /// disconnects them all.
    102 RemoveAppAccount { slug: String, id: String } => Call<()>
    /// The files the user sent in a session, other than pictures, by the
    /// `timestamp` of the `HistoryEntry::User` they came with. `GetFile`
    /// reads each one.
    103 GetSessionFiles { session_id: String } => Call<Vec<SentFiles>>
    /// Every tool an added server lists, as it last listed them, and which
    /// of them the agent gets.
    104 ListMcpTools { id: String } => Call<McpTools>
    /// Turns one of an added server's tools on or off for the agent. The
    /// server takes more than `McpTools::limit`; the app warns first.
    105 SetMcpTool { id: String, tool: String, enabled: bool } => Call<McpTools>
    /// The research a chat runs, with its log entries after `after` (0 for
    /// all), or `None` when the chat is not a research.
    106 GetResearch { session_id: Uuid, after: i64 } => Call<Option<Research>>
    /// Stops a research's agents. The chat and what they wrote stay.
    107 StopResearch { session_id: Uuid } => Call<()>
    /// A research's paper typeset as a PDF, with its sources. `NotFound`
    /// when the chat is not a research or its paper is not written yet.
    108 GetResearchPdf { session_id: Uuid } => Call<Upload>
    /// Every research of the account, newest first.
    109 ListResearches {} => Call<Vec<ResearchSummary>>
    /// A notice as a short voice note: `text` is it rewritten for listening,
    /// with no markdown or links, and `audio` that spoken. The app sends the
    /// `title` and `body` of a `Notice` it got, or a reply to read out with
    /// an empty `title`.
    110 SpeakNotice { title: String, body: String } => Call<VoiceNote>
    /// The machines running beep-node on the account, by name. A linked
    /// machine shows once it has connected.
    111 ListMachines {} => Call<Vec<Machine>>
    /// What each memory that is not about the user is about. Ids are the
    /// ones `ListMemories` gives; a memory missing here is about the user.
    112 ListMemoryScopes {} => Call<Vec<ScopedMemory>>
    /// Transcribes speech as it is said. After this frame the client sends
    /// the audio as more frames on the same stream, each a msgpack bin of
    /// mono 16-bit little-endian PCM at `sample_rate` (only 16000 is taken),
    /// and finishes its side when the speech ends. The server streams the
    /// words back as `Transcription::Delta`, then one `Done`, or only
    /// `Unavailable` when it has no live transcription. See `Duplex`.
    113 Transcribe { sample_rate: u32 } => Stream<Transcription>
    /// When each spoken prompt of the chat was sent: the `timestamp`s of
    /// its `HistoryEntry::User`s that were said aloud, oldest first.
    114 SpokenTurns { session_id: Uuid } => Call<Vec<i64>>
    /// `text` spoken as it is, with no rewrite, for live mode to read a
    /// reply out sentence by sentence. At most `MAX_SAY_CHARS` characters.
    115 Say { text: String } => Call<VoiceNote>
    /// A short line for live mode to say while a tool runs and the reply
    /// has said nothing for a while, such as "Let me check build-box".
    /// `step` is the tool step's label, `said` the user's message, whose
    /// language the line is in. `still` asks for the line that says the
    /// work goes on, after a long quiet.
    116 Filler { step: String, said: String, still: bool } => Call<VoiceNote>
    /// The sub-agents and background jobs the agent started in the chat:
    /// running ones first, with what they did last, then those that ended
    /// in the last hour, newest first. Unused: the agent no longer starts
    /// background work, so the server answers an empty list.
    117 ListWork { session_id: Uuid } => Call<Vec<WorkItem>>
    /// Stops a running sub-agent or job of the chat, and nothing comes back
    /// from it. `false` when it had already ended; `NotFound` when the chat
    /// has no such id. Unused: the server answers `NotFound`.
    118 CancelWork { session_id: Uuid, id: String } => Call<bool>
    /// The standing permissions the user gave routines that are still in
    /// force, newest first. A grant ends when it is revoked, when it
    /// expires and when its routine is deleted or ends itself.
    119 ListGrants {} => Call<Vec<Grant>>
    /// Ends a grant, by `Grant::id`, so its routine asks again. `NotFound`
    /// when the caller has no such grant.
    120 RevokeGrant { id: String } => Call<()>
    /// The questions background work in the chat is waiting on, oldest
    /// first: app actions of its sub-agents and jobs that ask first, each
    /// answered with `AnswerApproval`. `run_id` is empty. One that stops
    /// waiting, answered on any device or withdrawn, leaves the list and
    /// comes as a notice `Notice::resolved_question` reads.
    125 ListQuestions { session_id: Uuid } => Call<Vec<PendingApproval>>
    /// What each tool step of a session showed while it ran, by
    /// `HistoryTool::call_id` (`MessageBody::Tool::call_id` in updates), such
    /// as "Recalling memories". Steps saved before the server kept it are
    /// left out, as are steps that had none. `NotFound` when the caller has
    /// no such session.
    126 GetToolLabels { session_id: Uuid } => Call<Vec<ToolLabel>>
    /// Where Beep failed the caller, as the improvement watcher found it
    /// after their chats, most recently seen first: only those with `status`
    /// when given. Servers from before every user got findings answer
    /// `Unauthorized` to anyone but an admin.
    127 ListFindings { status: Option<FindingStatus> } => Call<Vec<Finding>>
    /// Resolves or dismisses a finding, by `Finding::id`, or opens it again.
    /// Nothing in Beep changes by itself. `NotFound` when the caller has no
    /// such finding.
    128 UpdateFinding { id: String, status: FindingStatus } => Call<Finding>
    /// The web pages each turn of a session used, by the `timestamp` of the
    /// `HistoryEntry::User` that started it, for the source chips under its
    /// reply. Turns that used none are left out. `NotFound` when the caller
    /// has no such session.
    129 GetSessionSources { session_id: Uuid } => Call<Vec<TurnSources>>
    /// What each machine has installed and can be asked to do, for the
    /// command center. A machine that has not reported yet has empty lists.
    130 ListMachineDetails {} => Call<Vec<MachineDetails>>
    /// The coding-agent sessions on the caller's online machines, newest
    /// activity first: those Beep started, and those started outside Beep
    /// that something runs now. A machine that does not answer in time is
    /// left out.
    131 ListAgentSessions {} => Call<Vec<MachineSession>>
    /// Answers the question a session Beep started asks, by
    /// `AgentQuestion::id`. `reply` is one of its options, by id or name;
    /// anything else declines and goes to the agent as its next prompt.
    /// `NotFound` when the machine is not one of the caller's online ones
    /// or has no such session, `Conflict` when the session no longer asks
    /// `question`.
    132 AnswerAgentQuestion { machine_id: String, agent: String, session: String, question: u32, reply: String } => Call<()>
    /// The approvals background work waits on in any of the caller's chats,
    /// oldest first. Answer one with `AnswerApproval`.
    133 ListPendingApprovals {} => Call<Vec<ChatApproval>>
    /// The project agent for `project` (`org/repo`), made with its chat on
    /// the first ask. `BadRequest` for a malformed name.
    134 GetProjectAgent { project: String } => Call<ProjectAgent>
    /// The tasks of a project, oldest first. `NotFound` when there is no
    /// such project.
    135 ListProjectTasks { project: Uuid } => Call<Vec<ProjectTask>>
    /// The workers of a project, including those that ended in the last
    /// day. `NotFound` when there is no such project.
    136 ListProjectWorkers { project: Uuid } => Call<Vec<ProjectWorker>>
    /// Moves a task to `Dropped`, or back to `Planned` to reopen it; any
    /// other state is a `BadRequest`. `NotFound` when there is no such
    /// task.
    137 UpdateProjectTask { task: Uuid, state: ProjectTaskState } => Call<ProjectTask>
    /// Stops a running worker. `NotFound` when there is no such worker.
    138 StopProjectWorker { worker: Uuid } => Call<()>
    /// Every project agent, for the command center.
    139 ListProjects {} => Call<Vec<ProjectAgent>>
    /// The caller's online machines that offer a coding agent, for
    /// starting one from the app.
    140 ListAgentHosts {} => Call<Vec<AgentHost>>
    /// Starts a coding-agent session on `prompt` in `cwd` (the machine's
    /// workspace or home folder when `None`). `machine_id` names the
    /// machine; without it the server picks one that offers `agent` (any
    /// when `None`) and has every capability in `needs` and those the
    /// prompt and folder call for, such as Xcode for an iOS app. `chat`, one
    /// of the caller's chats, then hears when a turn ends or the agent
    /// asks something. `NotFound` when the machine is not one of the
    /// caller's online ones or `chat` is not theirs; `BadRequest` saying
    /// why when no machine fits or the node refuses. `model`, an id from
    /// `ListAgentModels`, picks the agent's model; the agent's own default
    /// when `None`. `effort` sets how hard it thinks, one of
    /// `node::agent::efforts(agent)`; `BadRequest` for another. `checkout`
    /// first makes a branch or worktree in `cwd`'s git checkout, named
    /// from the prompt; `BadRequest` when the machine's beep-node is too old
    /// to. Older apps send none of these.
    141 StartAgentSession {
        machine_id: Option<String>,
        agent: Option<String>,
        cwd: Option<String>,
        prompt: String,
        needs: Vec<MachineCapability>,
        chat: Option<Uuid>,
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        effort: Option<AgentEffort>,
        #[serde(default)]
        checkout: Option<AgentCheckoutPlan>,
    } => Call<StartedAgent>
    /// One session as `ListAgentSessions` lists it, with its plan, the
    /// files it changed and the last `last` entries of its transcript (at
    /// most 200). An outside session has no plan or files. `NotFound` when
    /// the machine is not one of the caller's online ones or has no such
    /// session.
    142 GetAgentSession { machine_id: String, agent: String, session: String, last: u32 } => Call<AgentSessionView>
    /// Sends a session Beep started a message, stops its turn or ends it.
    /// A message waits for a running turn to end. `NotFound` as for
    /// `GetAgentSession`; `BadRequest` saying why when the node refuses.
    143 SteerAgentSession { machine_id: String, agent: String, session: String, steer: AgentSteer } => Call<()>
    /// Turns syncing `project` on or off on one machine. On, `folder` is
    /// its absolute folder there, inside the machine's shared folders;
    /// the machine then keeps it in step with the other machines that
    /// sync it, node to node. Off, the machine stops and its files stay.
    /// `NotFound` when the machine is not one of the caller's,
    /// `BadRequest` when `folder` is missing, the project is pinned, or the
    /// machine refuses the folder.
    150 SetProjectSync { project: String, machine_id: String, folder: Option<String>, on: bool } => Call<ProjectSync>
    /// Every project the caller syncs, with how each machine stands; an
    /// offline machine or one that does not answer in time shows
    /// `SyncStatus::Offline`.
    151 ListProjectSync {} => Call<Vec<ProjectSync>>
    /// What `project`'s sync leaves out on one machine (build output,
    /// ignored files, links), for the file viewer. `NotFound` when the
    /// machine does not sync it or is offline.
    152 GetSyncSkipped { project: String, machine_id: String } => Call<SyncSkipped>
    /// Pins a project to the machines it is on, or unpins it. A pinned
    /// project, such as a large game or video project, never syncs: every
    /// machine stops, and `SetProjectSync` refuses to turn it on. To pin
    /// one to the top of the sidebar instead, see `PinProjectToTop`.
    153 PinProject { project: String, pinned: bool } => Call<ProjectSync>
    /// What the event triage agent found worth telling the user and that
    /// they have not seen or dismissed yet, from the last two days, newest
    /// first, at most 20. Apps show them on their own, apart from any chat.
    160 ListUpdates {} => Call<Vec<AppUpdate>>
    /// Dismisses updates by `AppUpdate::id`, so they leave the list. Ids
    /// the caller does not have are skipped.
    161 DismissUpdates { ids: Vec<i64> } => Call<()>
    /// The caller's credentials: a GitHub token, a token per Forgejo or
    /// Gitea server, and API keys, one per provider. Never the secrets
    /// themselves, only their last four characters.
    162 ListCredentials {} => Call<Vec<Credential>>
    /// Saves a credential, replacing the caller's one of the same kind (for
    /// Forgejo, the same server). A GitHub or Forgejo token is checked with
    /// its host first, and an OpenRouter key with OpenRouter; `BadRequest`
    /// saying why when the host refuses it, or for an unknown provider or a
    /// bad server URL. Git tokens always go to the caller's machines; an API
    /// key only with `to_machines`, as its provider's env var.
    163 SaveCredential { kind: CredentialKind, secret: SecretText, to_machines: bool } => Call<Credential>
    /// Removes one of the caller's credentials, by `Credential::id`, from
    /// Beep and their machines. `NotFound` when the caller has no such one.
    164 RemoveCredential { id: String } => Call<()>
    /// What each of the caller's online machines last said it has free:
    /// disk, memory and load, sampled about once a minute. A machine that
    /// is offline or has not sent any yet is left out.
    165 ListMachineStats {} => Call<Vec<MachineStats>>
    /// Clones a repository onto one of the caller's machines and lists it
    /// as a project. `repo` is `org/repo` (GitHub), a GitHub URL, or a URL
    /// on a Forgejo or Gitea server in the caller's credentials.
    /// `machine_id` names the machine; without it, the caller's environment
    /// (the machine with a workspace), else the only one online. On Linux
    /// it goes to `<workspace or home>/projects/<org>/<repo>`, on macOS and
    /// Windows to `~/Projects/<org>/<repo>`; a folder that already holds
    /// that repository is used as it is. `folder` puts it somewhere else:
    /// an absolute path or one under the home folder (`~/code/repo`). A git
    /// checkout there is imported in place, an empty or missing folder gets
    /// the clone, and a folder with other files gets it in
    /// `<folder>/<repo>`. Apps from before it send no `folder`.
    /// `worktrees` are branches to give a worktree once it is in, as
    /// `AddBranchWorktree` does; one that fails is logged and the import
    /// still stands. Apps from before it send none.
    /// `BadRequest` saying why: the repo is malformed or on an unknown host,
    /// the folder is not one Beep takes, it is not found or the token has
    /// no access, the machine is offline, or the clone failed.
    166 ImportProject { repo: String, machine_id: Option<String>, #[serde(default)] folder: Option<String>, #[serde(default)] worktrees: Vec<BranchPlace> } => Call<ImportedProject>
    /// The projects the caller imported, by name, then machine; those on
    /// machines no longer linked are left out. `cloned` is always false.
    167 ListImportedProjects {} => Call<Vec<ImportedProject>>
    /// One folder on one of the caller's machines, for choosing where
    /// `ImportProject` puts a project: the folders in it and whether it is
    /// a git checkout. `path` is absolute or under the home folder
    /// (`~/code`); without it, the machine's workspace, else its home.
    /// `BadRequest` saying why: the machine is not one of the caller's, it
    /// is offline or runs no commands, or the path is malformed or not a
    /// folder.
    168 ListMachineFolder { machine_id: String, path: Option<String> } => Call<MachineFolder>
    /// Git checkouts the caller's coding agents worked in that are not
    /// imported yet, for the Add a project sheet: one row per repository,
    /// newest activity first, with its folder on each machine that has it
    /// and how many Claude Code, Codex, pi, flsh and OpenCode sessions ran
    /// there.
    /// Sessions in a subfolder or a worktree count for the checkout's main
    /// folder; the home folder and folders that are not checkouts are left
    /// out. A folder already imported on its machine, or a repository
    /// imported anywhere, is left out too, and so is a machine that is
    /// offline or does not answer in time.
    169 ListFoundFolders {} => Call<Vec<FoundProject>>
    /// The coding-agent sessions that ran in `project`'s folders on the
    /// caller's online machines, newest activity first, at most 50: those
    /// started outside Beep, such as Claude Code in a terminal. `title` is
    /// the session's title, else its first prompt. One nothing runs any
    /// more has status `Ended`; `ResumeAgentSession` picks it up. `NotFound`
    /// when the caller has not imported `project`.
    170 ListProjectSessions { project: String } => Call<Vec<MachineSession>>
    /// Resumes a session started outside Beep, by the agent's own id, as
    /// `ListProjectSessions` lists it: it then runs as a session Beep
    /// started, in its folder, on `prompt` when given. `NotFound` when the
    /// machine is not one of the caller's online ones; `BadRequest` saying
    /// why when the node refuses, such as when the session still runs in a
    /// terminal.
    171 ResumeAgentSession { machine_id: String, agent: String, session: String, prompt: Option<String> } => Call<StartedAgent>
    /// Every machine of the caller's, as `project`'s page lists it: its
    /// folder there, whether Beep is looking through it for a checkout now,
    /// and when it last did since the project was imported. A machine with
    /// a checkout first, then those online, each by name. Asking starts a
    /// look on an online machine not looked through since the import.
    /// Machines are looked through when a project is imported and when
    /// they connect: a checkout whose `origin` remote is the project's is
    /// listed as its folder there, in place. `NotFound` when the caller has
    /// not imported `project`. Admins only.
    172 ListProjectMachines { project: String } => Call<Vec<ProjectMachine>>
    /// The open issues on `project_id`'s issues host that no task of the
    /// project tracks and the caller has not dismissed, newest first, at
    /// most 100. Read with `gh` or `teacli` on the project's home checkout
    /// (else a machine it was imported on) on each call. A project with no
    /// GitHub or Forgejo host gives an empty list. `NotFound` when the
    /// caller has no such project; `BadRequest` with the host's error when
    /// the machine is offline or the command fails.
    173 ListProjectIssues { project_id: Uuid } => Call<Vec<ProjectIssue>>
    /// Makes a task that tracks the open issue `number`, without filing a
    /// new one. With `start`, a worker starts on it (or it waits for a free
    /// slot); without, the project agent is asked to plan it in the project
    /// chat. `NotFound` when the caller has no such project; `Conflict`
    /// when a task already tracks the issue; `BadRequest` saying why when
    /// the issue is not open, the project has no issues host, the agent or
    /// machine is unknown, or the host cannot be read.
    174 TrackProjectIssue { project_id: Uuid, number: u64, start: Option<IssueStart> } => Call<ProjectTask>
    /// Hides the issue `number` from `ListProjectIssues` for this project;
    /// it stays open on the host. `NotFound` when the caller has no such
    /// project.
    175 DismissProjectIssue { project_id: Uuid, number: u64 } => Call<()>
    /// A one-time setup token that links a machine to the caller's account:
    /// on the machine, `beep-node setup <token>` (see `setup::setup_command`).
    /// It works for ten minutes. `name` is what the machine is called once
    /// linked; without it, the name `setup` is given, else the machine's
    /// host name. `BadRequest` when the name is empty or too long or the
    /// server takes no machines.
    180 CreateNodeToken { name: Option<String> } => Call<NodeToken>
    /// Stops a token `CreateNodeToken` gave from working. `NotFound` when
    /// the caller made no such token or it was already used; `BadRequest`
    /// when `token` is not one.
    181 RevokeNodeToken { token: String } => Call<()>
    /// The `Machine::id` that claimed a token `CreateNodeToken` gave, or
    /// `None` while it is unused, so a sheet waiting on it knows which
    /// machine linked, even one set up again. `NotFound` when the caller
    /// made no such token; `BadRequest` when `token` is not one.
    182 GetNodeToken { token: String } => Call<Option<String>>
    /// The caller's open pull requests and issues on the git hosts in their
    /// credentials, newest activity first: the PRs they opened, those
    /// asking for their review, and the issues they opened or are assigned.
    /// Read with the saved tokens and kept for a few minutes, so asking
    /// again soon gives the same list. A host that fails is listed in
    /// `GitItems::hosts` with why, and the others still come.
    183 ListGitItems {} => Call<GitItems>
    /// The caller's GitHub accounts, the default one first, then oldest
    /// first. `ListCredentials` lists only the default one, as its single
    /// GitHub row, for apps from before several accounts.
    190 ListGitHubAccounts {} => Call<Vec<GitHubAccount>>
    /// Saves a GitHub token as one of the caller's accounts, after GitHub
    /// says who it signs in as: it replaces the account with that login,
    /// or adds one. The first account is the default. The orgs the token
    /// can see are kept, to pick the account for a project. `BadRequest`
    /// saying why when GitHub refuses it.
    191 SaveGitHubAccount { secret: SecretText } => Call<GitHubAccount>
    /// Makes one of the caller's GitHub accounts, by `GitHubAccount::id`,
    /// the default: the one machines get as `GH_TOKEN`. `NotFound` when the
    /// caller has no such account. Remove one with `RemoveCredential`.
    192 SetDefaultGitHubAccount { id: String } => Call<()>
    /// Pins `project` (`org/repo`) to the top of the sidebar, or unpins it.
    /// Unlike `PinProject`, which keeps a project off sync, this only sets
    /// where the apps list it. `BadRequest` for a malformed name.
    193 PinProjectToTop { project: String, pinned: bool } => Call<()>
    /// The projects the caller pinned to the top of the sidebar, in the
    /// order they were pinned.
    194 ListProjectsOnTop {} => Call<Vec<String>>
    /// Every project of the caller's with how many of its tasks stand in
    /// each state, its live workers and its latest task, by name: what the
    /// Projects overview shows, in one call instead of one per project.
    195 ListProjectOverview {} => Call<Vec<ProjectOverview>>
    /// The newest version of the caller's instructions for every agent
    /// Beep runs, or `None` before the first save.
    196 GetInstructions {} => Call<Option<InstructionsVersion>>
    /// Saves `text` as the next version of the caller's instructions, at
    /// most 16 KB. `base` is the version the text was edited from (`None`
    /// for the first); `Conflict` when a newer one was saved since, so an
    /// edit on another device is never overwritten. `note` says what made
    /// it, such as "Restored version 3"; empty for an edit.
    197 SaveInstructions { text: String, base: Option<u32>, note: String } => Call<InstructionsVersion>
    /// The versions of the caller's instructions, newest first, at most 50.
    198 ListInstructionsVersions {} => Call<Vec<InstructionsVersion>>
    /// The agent files on each of the caller's online machines: Claude
    /// Code's `~/.claude/CLAUDE.md`, Codex's `~/.codex/AGENTS.md`, Gemini
    /// CLI's `~/.gemini/GEMINI.md` and pi's `~/.pi/agent/AGENTS.md`, for
    /// each agent whose folder is there. A machine that cannot be read
    /// says why in `error`.
    199 ReadAgentFiles {} => Call<Vec<MachineAgentFiles>>
    /// Writes the newest instructions into each agent file on one machine,
    /// between `<!-- beep:start -->` and `<!-- beep:end -->`, for each
    /// agent whose folder is there; text outside the markers stays. Gives
    /// the files as they are after. `NotFound` when the machine is not one
    /// of the caller's online ones; `BadRequest` before the first save or
    /// when the machine refuses.
    200 SyncAgentFiles { machine_id: String } => Call<MachineAgentFiles>
    /// The terminals of `project` (`org/repo`): commands kept running in
    /// the background on the caller's machines, apart from any chat or
    /// app, oldest first, with how each stands now.
    201 ListTerminals { project: String } => Call<Vec<Terminal>>
    /// Starts `command` in `cwd` on one of the caller's online machines as
    /// a terminal of `project`, named `name` (the command when empty).
    /// `NotFound` when the machine is not one of the caller's online ones;
    /// `BadRequest` saying why when a field is malformed or the node
    /// refuses, such as a node from before background commands.
    202 StartTerminal { project: String, machine_id: String, name: String, command: String, cwd: String } => Call<Terminal>
    /// One terminal with the last of its output, at most 100 lines.
    /// `NotFound` when the caller has no such terminal.
    203 GetTerminal { id: Uuid } => Call<TerminalOutput>
    /// Stops a terminal's command when it still runs, then forgets the
    /// terminal. `NotFound` when the caller has no such terminal;
    /// `BadRequest` when its machine is online and refuses to stop it.
    204 CloseTerminal { id: Uuid } => Call<()>
    /// `project`'s (`org/repo`) branches on one machine with a checkout
    /// (the home checkout's when it is online, else another online one):
    /// local branches and those only on `origin`, newest commit first, at
    /// most 100, each with its worktree there, how far it is ahead of and
    /// behind the default branch, and its last commit. Read with git on
    /// each call, without fetching. `NotFound` when the caller has no such
    /// project; `BadRequest` saying why when no machine with a checkout is
    /// online or git fails.
    205 ListProjectBranches { project: String } => Call<ProjectBranches>
    /// Reviews `branch` of `project` against its default branch before a
    /// merge: fetches, reads the diff's stat, whether it merges cleanly
    /// and the branch's pull request with its checks, then starts one
    /// read-only coding-agent session per shard of the diff (by top-level
    /// area). `NotFound` when the caller has no such project; `BadRequest`
    /// saying why when no machine with a checkout and a coding agent is
    /// online, the branch is not found, it has no changes or an agent does
    /// not start.
    206 StartBranchReview { project: String, branch: String } => Call<BranchReview>
    /// One review as it stands. Asking moves it on: a reviewer that ended
    /// its turn gives its findings and a fresh verifier session checks
    /// them. `NotFound` when the caller has no such review.
    207 GetBranchReview { id: Uuid } => Call<BranchReview>
    /// `project`'s newest review of each branch, newest first, at most 20,
    /// as they stand without moving them on.
    208 ListBranchReviews { project: String } => Call<Vec<BranchReview>>
    /// Dismisses one finding of a review, by `ReviewFinding::id`, or brings
    /// it back. `NotFound` when the caller has no such review or finding.
    209 DismissReviewFinding { review: Uuid, finding: u32, dismissed: bool } => Call<BranchReview>
    /// Makes a project task with the review's open findings and starts a
    /// worker on it, in a worktree off the reviewed branch. `NotFound` when
    /// the caller has no such review; `BadRequest` when nothing is open or
    /// the review is still running.
    210 FixReviewFindings { review: Uuid } => Call<ProjectTask>
    /// Asks the project agent, in the project chat, to post the review's
    /// open findings on the branch's pull request; its host tools ask the
    /// user before anything posts. Gives the project chat's id. `NotFound`
    /// when the caller has no such review; `BadRequest` when the branch has
    /// no pull request or the review is still running.
    211 PostBranchReview { review: Uuid } => Call<Uuid>
    /// Asks the project agent, in the project chat, to merge the branch's
    /// pull request; the merge always asks the user first. Gives the
    /// project chat's id. `NotFound` when the caller has no such review;
    /// `BadRequest` saying why while the review runs, findings are open,
    /// checks fail, the branch conflicts or it has no pull request.
    212 MergeReviewedBranch { review: Uuid } => Call<Uuid>
    /// How the caller's coding agents sign in on their machines, for the
    /// Agent accounts section of Settings: the Claude Code token and Codex
    /// API key Beep keeps (never the secrets) and each machine that has
    /// Claude Code or Codex.
    213 GetAgentAccounts {} => Call<AgentAccounts>
    /// Saves the token `agent` (an `AgentAccount` slug: `claude` or
    /// `codex`) uses on the caller's machines, replacing the one before.
    /// The machines get it as the agent's env var when they start it.
    /// `BadRequest` for an unknown agent or a secret that is not a token.
    214 SaveAgentToken { agent: String, secret: SecretText } => Call<AgentToken>
    /// Removes `agent`'s token from Beep and the caller's machines; running
    /// sessions keep it until they end. `NotFound` when there is none.
    215 RevokeAgentToken { agent: String } => Call<()>
    /// Starts Codex's device sign-in (`codex login --device-auth`) on one
    /// of the caller's online machines and answers with the page and code
    /// it printed. `NotFound` when the machine is not one of the caller's
    /// online ones; `BadRequest` saying why when Codex cannot start it.
    216 StartCodexLogin { machine_id: String } => Call<CodexLogin>
    /// How a sign-in `StartCodexLogin` started stands.
    217 GetCodexLogin { machine_id: String, task: u32 } => Call<CodexLoginState>
    /// Stops a sign-in `StartCodexLogin` started, when it still waits.
    218 CancelCodexLogin { machine_id: String, task: u32 } => Call<()>
    /// The machine `StartAgentSession` without a `machine_id` would start
    /// these fields on now, and why, without starting anything: what the
    /// start form shows under Auto. `BadRequest` saying why when no machine
    /// fits, as the start would.
    219 PreviewAgentMachine { agent: Option<String>, cwd: Option<String>, prompt: String, needs: Vec<MachineCapability> } => Call<MachinePick>
    /// The caller's open pull requests and the open issues they opened on
    /// `project_id`'s GitHub repository, newest activity first, each saying
    /// whether someone else wrote after the caller's last comment or
    /// review. Read with `gh` on the project's home checkout (else a
    /// machine it was imported on) on each call. A project with no GitHub
    /// host gives an empty list. `NotFound` when the caller has no such
    /// project; `BadRequest` with the host's error when the machine is
    /// offline or the command fails.
    220 ListProjectThreads { project_id: Uuid } => Call<Vec<ProjectThread>>
    /// Asks the project agent, in the project chat, to read the pull
    /// request or issue `number` of `repo` and draft a reply; posting it
    /// always asks the user first. Gives the project chat's id. `NotFound`
    /// when the caller has no such project; `BadRequest` saying why when
    /// `repo` is not the project's or the thread cannot be read.
    221 ReplyToThread { project_id: Uuid, repo: String, number: u64 } => Call<Uuid>
    /// The chat about one pull request or issue of `project_id`, made on
    /// the first ask: the same chat on every call for one thread. It runs
    /// as the project agent, reading the thread again at the start of each
    /// turn. `NotFound` when the caller has no such project; `BadRequest`
    /// saying why when `repo` is not the project's or the thread cannot be
    /// read.
    222 OpenThreadChat { project_id: Uuid, repo: String, number: u64 } => Call<ThreadChat>
    /// The chats about one pull request or issue of `project_id`, newest
    /// first. `NotFound` when the caller has no such project.
    223 ListThreadChats { project_id: Uuid } => Call<Vec<ThreadChat>>
    /// Where a reviewed branch is now: fetches it on the review's machine
    /// and says whether it moved since the review (a new local commit, or
    /// a push the review did not read). Merging a stale review is refused.
    /// `NotFound` when the caller has no such review; `BadRequest` saying
    /// why when the machine is offline or git fails.
    240 CheckBranchReview { id: Uuid } => Call<ReviewHead>
    /// What importing `repo` (`org/repo` or its URL) would find, before
    /// `ImportProject`: the checkouts of it on the caller's online machines,
    /// found by any remote (`origin` or a fork's), and its branches with
    /// work in the first one: the heads of the caller's open pull requests,
    /// branches ahead of the default branch and those with a worktree, the
    /// default branch last. Read on each call. Without a checkout, only the
    /// pull requests, read with `gh` on an online machine. `BadRequest`
    /// saying why when `repo` is malformed or no machine is online.
    245 PreviewImport { repo: String } => Call<ImportPreview>
    /// Gives `branch` of `project` (`org/repo`) a worktree on the machine
    /// `machine_id`, at `.worktrees/<branch>` in its checkout there (a `/`
    /// in the name becomes `-`), fetching the branch from a remote that has
    /// it when it is not local. Gives the branch as it is after. `NotFound`
    /// when the caller has no such project; `BadRequest` saying why when
    /// the machine is offline or has no checkout of it, the branch already
    /// has a worktree or is not found, or git fails.
    246 AddBranchWorktree { project: String, branch: String, machine_id: String } => Call<ProjectBranch>
    /// Always empty now: the server keeps no images of coding agent
    /// sessions. Read each `[image: image-<key>]` line's picture with
    /// `GetAgentImage`. Kept for the apps that still ask.
    247 ListAgentImages { machine_id: String, session: String } => Call<Vec<AgentImage>>
    /// The caller's project groups, oldest first: folders the apps list
    /// projects under, in place of their org, on every machine.
    248 ListProjectGroups {} => Call<Vec<ProjectGroup>>
    /// Saves a project group: a new one without `id`, else the caller's
    /// group `id` with its new name and folders. Each folder is kept as
    /// `groups::project_group_path` gives it, so `~/Projects/work`
    /// and `/home/user/work` are one folder, and repeats are
    /// dropped. An empty `name` takes the first folder's. `BadRequest`
    /// saying why when there is no folder or more than 16, one is a home
    /// folder or the root, or a field is too long; `NotFound` when the
    /// caller has no group `id`.
    249 SaveProjectGroup { id: Option<Uuid>, name: String, paths: Vec<String> } => Call<ProjectGroup>
    /// Forgets one of the caller's project groups; its projects go back
    /// under their orgs. `NotFound` when the caller has no such group.
    250 DeleteProjectGroup { id: Uuid } => Call<()>
    /// Unlinks one of the caller's machines, by `Machine::id`, online or
    /// not: the server forgets it and stops syncing with it, drops its
    /// connection if it has one, and refuses its node until it is set up
    /// again. Any key of the account may remove a machine, where
    /// `RemoveDevice` needs the primary key. `NotFound` when no machine of
    /// the caller's has that id; `BadRequest` for the caller's own key,
    /// which `RemoveDevice` unlinks.
    251 RemoveMachine { machine_id: String } => Call<()>
    /// Removes `project` (`org/repo`) from Beep for the caller, deleting no
    /// files on any machine: its imports go, every machine stops syncing it
    /// (as `SetProjectSync` with `on` false does), its live workers and
    /// terminals are stopped, and its agent chat, thread chats, tasks,
    /// workers, branch reviews, pins and hidden issues are deleted. Issues
    /// and pull requests on its host stay as they are. Importing it again
    /// starts it afresh. `NotFound` when the caller has no such project.
    252 RemoveProject { project: String } => Call<()>
    /// The models the coding agent `agent` can run on `machine_id`, for
    /// `StartAgentSession`'s `model`, read from the node, which keeps them
    /// a few minutes. `AgentModelPick::Fixed` when its beep-node is too old
    /// to pick one. `NotFound` when the machine is not one of the caller's
    /// online ones; `BadRequest` saying why when it has no such agent or
    /// does not answer.
    253 ListAgentModels { machine_id: String, agent: String } => Call<AgentModels>
    /// The picture of a `[image: image-<key>]` line in the log of the
    /// coding agent session `session` on `machine_id`, read from the node
    /// on each call; the server keeps nothing. `NotFound` when the caller
    /// has no such online machine or the node no longer has the image;
    /// `BadRequest` saying why when the node cannot be asked.
    260 GetAgentImage { machine_id: String, session: String, key: String } => Call<AgentImageData>
    /// The caller's machines on which `project`'s folder is kept in step by
    /// Syncthing, as each machine last reported its Syncthing folders. Beep
    /// does not sync such a folder: `SetProjectSync` refuses it, and a
    /// machine already syncing it stops. Empty when there is none or the
    /// caller has not imported `project`.
    261 ListProjectSyncthing { project: String } => Call<Vec<SyncthingPlace>>
    /// Every open pull request on the project's issues host, anyone's,
    /// newest first: the 100 newest on GitHub, 25 on Forgejo. Empty when
    /// the project has no GitHub or Forgejo host; `BadRequest` saying why
    /// when no machine of the project can read it or the host refuses.
    262 ListProjectPullRequests { project_id: Uuid } => Call<Vec<ProjectPullRequest>>
    /// The branch the open pull request `number` of `project_id` is made
    /// from, read from its host now, for Continue. `NotFound` when the
    /// caller has no such project; `BadRequest` saying why when no machine
    /// of the project can read it, the host refuses, or there is no such
    /// pull request.
    299 GetPullRequestHead { project_id: Uuid, number: u64 } => Call<PullRequestHead>
    /// The runs going on in any of the caller's sessions, started from any
    /// of their devices, so a chat list can mark the chats still
    /// answering. Empty when none runs.
    285 ListActiveRuns {} => Call<Vec<Run>>
    /// One pull request or issue of `project_id` read from its host now:
    /// its opening post, its newest comments and reviews, and whether it
    /// waits on the caller's reply, for the card atop a chat about it.
    /// `NotFound` when the caller has no such project; `BadRequest` saying
    /// why when `repo` is not the project's or the thread cannot be read.
    286 GetProjectThread { project_id: Uuid, repo: String, number: u64 } => Call<ThreadView>
    /// The coding agent harnesses on the caller's machine `machine_id`, for
    /// the Coding agents page: what its node finds of each (version, the
    /// newest one, where it is, its sign-in) with the caller's settings for
    /// it. `refresh` looks the newest versions up again. `NotFound` when
    /// the machine is not one of the caller's online ones; `BadRequest`
    /// saying why when its beep-node is too old or does not answer.
    264 ListAgentHarnesses { machine_id: String, refresh: bool } => Call<AgentHarnesses>
    /// Saves the caller's settings for the harness `agent` on `machine_id`,
    /// replacing the ones before, and sends them to the machine when it is
    /// online. An env var whose `value` is `None` keeps the value saved
    /// before, and is dropped when there was none. A harness turned off is
    /// not offered on that machine, by Auto or by name. `NotFound` when the
    /// machine is not one of the caller's; `BadRequest` saying why for an
    /// unknown agent or a field that is too long or malformed.
    265 SaveHarnessSettings { machine_id: String, agent: String, settings: HarnessSettings } => Call<()>
    /// Installs the harness `agent` on `machine_id`, or updates it to the
    /// newest version, with the command its node names, and gives the
    /// harnesses as they stand after. It can take a few minutes. `NotFound`
    /// when the machine is not one of the caller's online ones;
    /// `BadRequest` saying why when the node knows no way to install it or
    /// the command fails.
    266 UpdateAgentHarness { machine_id: String, agent: String } => Call<AgentHarnesses>
    /// How `project`'s sync stands on each machine that syncs it, in more
    /// detail than `ListProjectSync`: whether it is paused, what a machine
    /// still has to bring, how fast it moves and how long that takes. An
    /// offline machine shows `SyncStatus::Offline` and no progress.
    290 GetProjectSyncDetail { project: String } => Call<ProjectSyncDetail>
    /// Pauses `project`'s sync on every machine, or resumes it. A paused
    /// machine keeps its files and its list, and catches up when resumed;
    /// an older beep-node stops the project instead and scans it afresh
    /// when resumed. `BadRequest` when the project is pinned.
    291 PauseProjectSync { project: String, paused: bool } => Call<ProjectSyncDetail>
    /// Settles one conflict of `project` on `machine_id`: `Copy` keeps the
    /// conflict copy at `path` as the file it stands beside, `Original`
    /// keeps that file and deletes the copy. The machine changes its folder
    /// and syncs the result to the others. `NotFound` when the machine does
    /// not sync the project or is offline; `BadRequest` saying why when
    /// its beep-node is too old, sync is paused, or `path` is not a
    /// conflict copy there.
    292 ResolveSyncConflict { project: String, machine_id: String, path: String, keep: ConflictKeep } => Call<()>
    /// Starts an interactive terminal of `project` (none when empty) on one
    /// of the caller's online machines: `command` in `cwd` (the machine's
    /// workspace, else its home folder, when empty), or the user's shell
    /// there when `command` is empty, in a pseudo-terminal of `cols` by `rows` cells,
    /// named `name` (the command, or "shell", when empty). `AttachTerminal`
    /// follows it and types into it; `CloseTerminal` stops it. `NotFound`
    /// when the machine is not one of the caller's online ones;
    /// `BadRequest` saying why when a field is malformed or the node
    /// refuses, such as a node from before interactive terminals.
    272 OpenTerminal { project: String, machine_id: String, name: String, command: String, cwd: String, cols: u32, rows: u32 } => Call<Terminal>
    /// Follows a terminal live from byte `from` of its output, at `cols` by
    /// `rows` cells: `TerminalEvent::Output` as it comes, until it ends or
    /// the client finishes its side. After the request the client sends
    /// `TerminalInput`s (see `Duplex`). A terminal `StartTerminal` started
    /// cannot be typed into: it answers `ViewOnly` and ends. `NotFound`
    /// when the caller has no such terminal.
    273 AttachTerminal { id: Uuid, from: u64, cols: u32, rows: u32 } => Stream<TerminalEvent>
    /// Every terminal of the caller on every machine, of any project or of
    /// none (`project` empty), oldest first, with how each stands now.
    /// Terminals of an offline machine are `Offline`.
    293 ListAllTerminals {} => Call<Vec<Terminal>>
    /// What the caller's coding agents cost today and this week, by
    /// project, and how much of each agent's plan is left, for the pill
    /// atop the command center. The plans are read on the caller's
    /// machines every few minutes; `refresh` asks the online ones to read
    /// them again now, and the answer comes before they did.
    294 GetUsage { refresh: bool } => Call<UsageView>
    /// Whether importing `repo` (`org/repo` or its URL) clones the
    /// caller's fork of it rather than the repository itself: their fork on
    /// the same host, found with their token there, and the head branches
    /// of their open pull requests from it, which the import makes local
    /// branches. The import adds the repository as the remote `upstream`.
    /// `fork` is `None` when they have no fork, no token for the host, or
    /// the host could not be read. Read on each call. `BadRequest` saying
    /// why when `repo` is malformed.
    295 FindImportFork { repo: String } => Call<ImportFork>
    /// Part of a media file on the caller's online machine `machine_id`,
    /// such as a video, 3D model, PDF or picture a coding agent made.
    /// `path` is absolute or starts `~/` (the node's home). Gives up to
    /// [`AGENT_FILE_CHUNK`] bytes from `offset`, with the file's whole size
    /// and MIME type; the app asks again from `offset` plus the bytes it
    /// got until it has `size`. Only files whose extension [`media_mime`]
    /// knows and of at most [`MAX_AGENT_FILE`] bytes. The server keeps
    /// nothing. `NotFound` when the machine is not one of the caller's
    /// online ones or the file is not there; `BadRequest` saying why for
    /// another type, a file too large, or a node too old to read files
    /// (updating beep-node may help).
    300 GetAgentFile { machine_id: String, path: String, offset: u64 } => Call<AgentFileChunk>
    /// One folder of a coding agent session's working folder `root` on the
    /// caller's online machine `machine_id`, for the session's Files tab.
    /// `root` is absolute or starts `~/`; `path` is the folder inside it,
    /// relative with `/` between names, `""` for `root` itself. Leaves out
    /// what git ignores unless `ignored`, and `.git` always; marks what git
    /// sees changed. `BadRequest` saying why for a path outside `root` (a
    /// symlink out of it included), a folder that is not there, or a node
    /// too old to list folders (updating beep-node may help); `NotFound`
    /// when the machine is not one of the caller's online ones.
    301 ListWorkFolder { machine_id: String, root: String, path: String, ignored: bool } => Call<WorkFolder>
    /// Up to `len` bytes, at most [`AGENT_FILE_CHUNK`], of the file `path`
    /// inside `root` from `offset`, as `ListWorkFolder` names them: any
    /// kind of file, for the Files tab to show or save. `mime` is
    /// [`media_mime`]'s, else `application/octet-stream`. `NotFound` when
    /// the file or the machine is not there; `BadRequest` as for
    /// `ListWorkFolder`.
    302 ReadWorkFile { machine_id: String, root: String, path: String, offset: u64, len: u32 } => Call<AgentFileChunk>
    /// Writes part of a file into `root`, for files dropped on a session.
    /// `path` is where it goes inside `root`; the first part (`offset` 0)
    /// makes the folders on the way and takes a free name beside the one
    /// asked (`notes 2.txt`), and the reply is the path it took, which the
    /// later parts send. Parts come in order, each at most
    /// [`AGENT_FILE_CHUNK`], until `offset` plus the part is `size`, at
    /// most [`MAX_WORK_UPLOAD`]; only then does the file show under its
    /// name. Never replaces a file. `BadRequest` saying why for a path
    /// outside `root`, a part out of order, a file too large, or a node too
    /// old to take files.
    303 WriteWorkFile { machine_id: String, root: String, path: String, offset: u64, size: u64, #[serde(with = "serde_bytes")] data: Vec<u8> } => Call<String>
    /// What the caller's coding agents cost today, this week (from Monday)
    /// and this month, in the caller's timezone, by project and by
    /// harness, and how much of each agent's plan is used. Unlike
    /// `GetUsage` it counts sessions started outside Beep too, as the
    /// machines read them from the agents' own logs. `refresh` as in
    /// `GetUsage`.
    304 GetUsageReport { refresh: bool } => Call<UsageReport>
    /// The chat about the update `id`, as `ListUpdates` gave it, made on
    /// the first ask and the same chat after. A new chat opens with Beep
    /// telling the user the update, so they can reply; Beep answers with
    /// the update's events in its context. Opening it dismisses the update.
    /// `NotFound` when the caller has no such update, or it is gone.
    305 OpenUpdateChat { id: i64 } => Call<Session>
    /// The custom agents the coding agent harnesses define for the project
    /// folder `folder` on the caller's online machine `machine_id`, then
    /// the user's own there: Claude Code's `.claude/agents`, OpenCode's
    /// `.opencode/agent` and `opencode.json`, Codex's `.codex/agents`,
    /// pi's `.pi/agents` and flsh's `.agents`, each in the folder and in
    /// the harness's folder in the node's home. `folder` is absolute or
    /// starts `~/`. A file that does not read gives a row with `error`
    /// set. `NotFound` when the machine is not one of the caller's online
    /// ones; `BadRequest` saying why when the folder is not there or the
    /// node is too old to read custom agents (updating beep-node may help).
    306 ListCustomAgents { machine_id: String, folder: String } => Call<Vec<CustomAgent>>
    /// The open pull requests and the open issues no task tracks of
    /// `project_id`, as `ListProjectPullRequests` and `ListProjectIssues`
    /// give them, from the server's copy and with when it was read. The
    /// server reads the host on the first ask, and every few minutes while
    /// someone looks at the project. A copy a few minutes old is given as
    /// it is and read again in the background; `WatchProject` says when. `refresh` reads it now, at most once per 30 seconds per
    /// project; sooner, the copy is given as it is. A host that could not
    /// be read gives its error and the lists read before. `NotFound` when
    /// the caller has no such project.
    307 GetProjectLists { project_id: Uuid, refresh: bool } => Call<ProjectLists>
    /// The newest workflow runs (GitHub Actions or Forgejo Actions) of
    /// `project_id`'s repository, newest first, with how far a running one
    /// got and the checks of each open pull request. Read from the server's
    /// copy as `GetProjectLists` keeps it; while a run is in progress and
    /// someone asks, its jobs are read again at most every 10 seconds.
    /// `refresh` as in `GetProjectLists`. A host that cannot be read, or
    /// that limits the caller's token, says so in the reply. `NotFound`
    /// when the caller has no such project.
    308 ListProjectRuns { project_id: Uuid, refresh: bool } => Call<ProjectRuns>
    /// One workflow run of `project_id` with its jobs and steps and, when
    /// it failed, the end of its failing job's log. Read as
    /// `ListProjectRuns` reads jobs. `NotFound` when the caller has no such
    /// project; `BadRequest` saying why when the host cannot be read or has
    /// no such run.
    309 GetProjectRun { project_id: Uuid, run_id: u64 } => Call<ProjectRunDetail>
    /// As `GetUsageReport`, with what a subscription covered apart: dollars
    /// count only metered API spend (API keys, OpenRouter, Bedrock and the
    /// like), and use a Claude, ChatGPT or Copilot plan covered counts as
    /// tokens. A machine that cannot tell counts Claude Code and Codex as
    /// subscription, the other harnesses as metered.
    310 GetSpendReport { refresh: bool } => Call<SpendReport>
    /// Follows `project_id` while its page is open: the first reply says
    /// whether its host's events reach Beep now, and each later one comes
    /// when the server read its lists or runs again, or when that changed.
    /// While someone follows a project whose repository the user is an
    /// admin of, one of their machines forwards its webhook. The page
    /// keeps reading the lists every few minutes as before. `NotFound`
    /// when the caller has no such project.
    340 WatchProject { project_id: Uuid } => Stream<ProjectChange>
    /// As `ListProjectRuns`, from every host `project_id`'s repository is
    /// on: its GitHub and Forgejo hosts, from its roles, its imports and
    /// its checkout's remotes. One push to a repository mirrored on both
    /// runs on both, so the runs of each are merged newest first and each
    /// names its host. A host that cannot be read gives its own error in
    /// `HostedRuns::hosts` and the others still show. `ListProjectRuns`
    /// gives the primary host's alone. `NotFound` when the caller has no
    /// such project.
    360 ListProjectRunsAll { project_id: Uuid, refresh: bool } => Call<HostedRuns>
    /// As `GetProjectRun`, for the run `run_id` on `host`, as
    /// `HostedRun::host` names it. `BadRequest` saying why when the
    /// project's repository is not on that host.
    361 GetProjectRunOn { project_id: Uuid, host: String, run_id: u64 } => Call<ProjectRunDetail>
    /// The caller's flsh settings (provider routing and compaction), the
    /// same on every machine. All unset until they save some.
    370 GetFlshSettings {} => Call<FlshSettings>
    /// Saves the caller's flsh settings, replacing the ones before, and
    /// sends them to every machine of theirs online; each writes them to
    /// flsh's config before it starts flsh. `BadRequest` saying why for a
    /// value flsh would not take (`FlshSettings::check`).
    371 SaveFlshSettings { settings: FlshSettings } => Call<()>
    /// The caller's settings for the harness `agent` on each of their
    /// machines, online or not, so the Coding agents page can say which
    /// fields differ between them. A machine with none saved gives the
    /// defaults. Env var values never come back.
    372 ListHarnessSettings { agent: String } => Call<Vec<MachineHarnessSettings>>
    /// Saves `settings` for the harness `agent` on every machine of the
    /// caller's, online or not: online ones get them now, the others when
    /// they next connect. Each machine keeps whether the harness is on, and
    /// its own binary path unless `include_program`. An env var whose
    /// `value` is `None` takes the value saved on `from_machine`; each
    /// machine's copy is sealed for that machine. Replies how many machines
    /// were saved. `NotFound` when `from_machine` is not one of the
    /// caller's; `BadRequest` as `SaveHarnessSettings`.
    373 SaveHarnessSettingsAll { from_machine: String, agent: String, settings: HarnessSettings, include_program: bool } => Call<u32>
    /// What changed in the git checkout `root` on the caller's online
    /// machine `machine_id`, for a coding agent session's Changes tab: the
    /// diff from where `head` left `base` to `head`. `base` is a branch,
    /// `""` for the checkout's default (`origin/HEAD`, else `main` or
    /// `master`); `head` is `""` for the working tree, so uncommitted
    /// changes and untracked files count, else a branch: its worktree
    /// inside `root` read the same way when it has one, else its commits.
    /// Each file gives at most [`MAX_DIFF_FILE_LINES`] lines and all of
    /// them at most [`MAX_DIFF_LINES`]; past that, and for lockfiles
    /// unless `lockfiles`, a file comes without its lines and says why.
    /// `only` names files to give in full past those caps, up to
    /// [`MAX_DIFF_SHOWN_LINES`] each, and then the reply has only them.
    /// `root` is reached as `ListCustomAgents` reaches its folder.
    /// `NotFound` when the machine is not one of the caller's online ones;
    /// `BadRequest` saying why for a folder outside the node's reach, one
    /// not in a checkout, a branch git does not know, or a node too old to
    /// read diffs (updating beep-node may help).
    380 GetWorkDiff { machine_id: String, root: String, base: String, head: String, only: Vec<String>, lockfiles: bool } => Call<WorkDiff>
    /// As `GetWorkDiff`, for `branch` of `project` (`org/repo`) against
    /// its default branch, read on a machine with a checkout as
    /// `ListProjectBranches` picks it. `NotFound` when the caller has no
    /// such project; `BadRequest` saying why when no machine with a
    /// checkout is online or the node refuses.
    381 GetBranchDiff { project: String, branch: String, only: Vec<String>, lockfiles: bool } => Call<WorkDiff>
    /// Checks the caller's saved Forgejo or Gitea token `id` (a
    /// `Credential::id`) from where its server can be reached: Beep's
    /// server when it can reach it, else the caller's online machines with
    /// teacli, which hold the token in teacli's config. A login found is
    /// kept as the credential's `account`. `NotFound` when the caller has
    /// no such Forgejo credential.
    382 CheckCredential { id: String } => Call<CredentialCheck>
    /// `path` inside the working folder `root` and the folders under it,
    /// `depth` levels down in all (1 is `path` alone, at most
    /// [`MAX_TREE_DEPTH`]), as `ListWorkFolder` lists each, in one ask.
    /// Folders git ignores are listed but not read into, and the walk stops
    /// past [`MAX_TREE_ENTRIES`] entries. A node from before it gives
    /// `path` alone. Errors as for `ListWorkFolder`.
    383 ListWorkTree { machine_id: String, root: String, path: String, ignored: bool, depth: u32 } => Call<WorkTree>
    /// Has one of the caller's online machines install the newest
    /// beep-node over itself and restart, as `beep-node update` does
    /// there. It holds new prompts and restarts once its agent sessions
    /// are idle, or after a wait; `stop_sessions` restarts at once. The
    /// sessions resume after the restart. The reply comes once it is
    /// installed, before the restart. `NotFound` when the machine is not
    /// the caller's; `BadRequest` saying why when it is offline, too old to
    /// be updated this way, or the update failed.
    390 UpdateMachine { machine_id: String, stop_sessions: bool } => Call<MachineUpdate>
    /// The caller's online machines that `UpdateMachine` can update. A
    /// machine missing here is updated on the machine.
    391 ListUpdatableMachines {} => Call<Vec<UpdatableMachine>>
    /// Pull request or issue `number` of `project_id` read from its GitHub
    /// or Forgejo host now, for the project page: its state, labels, opening
    /// post and comments, and for a pull request its reviews and checks.
    /// `NotFound` when the caller has no such project; `BadRequest` saying
    /// why when the project has no host or the host cannot be read.
    392 GetThreadDetail { project_id: Uuid, number: u64 } => Call<ThreadDetail>
}

/// A machine `UpdateMachine` can update (`ListUpdatableMachines`).
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdatableMachine {
    /// Its `Machine::id`.
    pub id: String,
    /// The agent sessions running on it, which an update waits for.
    pub agent_sessions: u32,
}

/// What `UpdateMachine` did.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineUpdate {
    /// The beep-node version it ran.
    pub from: String,
    /// The version installed, or `from` when that is already the newest.
    pub to: String,
    /// It restarts on `to`: it goes offline and connects again shortly.
    pub restarting: bool,
    /// Why it does not restart, in words for the user; empty otherwise.
    pub note: String,
}

/// One machine's settings for a harness (`ListHarnessSettings`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MachineHarnessSettings {
    pub machine_id: String,
    pub machine_name: String,
    pub settings: HarnessSettings,
}

/// What `ListProjectRunsAll` gives.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostedRuns {
    /// Every host's runs, newest first.
    pub runs: Vec<HostedRun>,
    /// How the newest runs of each open pull request stand, per host: a
    /// pull request's head pushed to both hosts has checks on both.
    pub checks: Vec<HostedChecks>,
    /// The hosts read, the primary one first.
    pub hosts: Vec<RunsHost>,
    /// Why no host could be read at all, such as a project with no GitHub
    /// or Forgejo host.
    pub error: Option<String>,
}

/// A run and the host it ran on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostedRun {
    /// As `RunsHost::host`.
    pub host: String,
    pub run: WorkflowRun,
}

/// A pull request's checks on one host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostedChecks {
    /// As `RunsHost::host`.
    pub host: String,
    pub checks: PrChecks,
}

/// One host a project's runs are read from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunsHost {
    /// `github.com`, or a Forgejo server's host, such as `forge.example.com`.
    pub host: String,
    pub kind: GitHostKind,
    /// `org/repo` on that host.
    pub repo: String,
    /// The host the project's issues and pull requests are on, the one
    /// `ListProjectRuns` reads.
    pub primary: bool,
    /// When its run list was last read.
    pub synced_at: Option<SystemTime>,
    /// Why its runs could not be read; its runs are the ones read before.
    pub error: Option<String>,
    /// As `ProjectRuns::rate_limited_until`, for this host.
    pub rate_limited_until: Option<SystemTime>,
}

/// What `WatchProject` streams.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectChange {
    /// The host's events reach Beep, so the lists and runs are read again
    /// within seconds of a change. False while they are read every few
    /// minutes only.
    pub live: bool,
    /// Why the project is not live, for the user; `None` when it is, or
    /// while that is not known yet.
    pub polling_why: Option<String>,
    /// The pull request and issue lists were read again since the last
    /// reply.
    pub lists: bool,
    /// The workflow runs were read again since the last reply.
    pub runs: bool,
}

/// One custom agent a coding agent harness defines, as
/// `ListCustomAgents` reads it: a named prompt with a description, and
/// optionally the model, tools and color it runs with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomAgent {
    /// The name the harness knows it by; the file's stem when it names
    /// none.
    pub name: String,
    pub harness: CustomAgentHarness,
    pub scope: CustomAgentScope,
    pub description: String,
    /// As the file names it, such as `opus` or `inherit`.
    pub model: Option<String>,
    /// The tools it may use, as the file lists them; empty for all.
    pub tools: Vec<String>,
    /// As the file names it, such as `blue` or `#3b82f6`.
    pub color: Option<String>,
    /// The file it came from: relative to the project folder for a
    /// project agent, starting `~/` for a user one.
    pub path: String,
    /// Its prompt, at most [`MAX_CUSTOM_AGENT_PROMPT`] bytes.
    pub prompt: String,
    /// The prompt goes on past what `prompt` holds.
    pub prompt_cut: bool,
    /// Why the file could not be read, when it could not; the other fields
    /// then say what was found.
    pub error: Option<String>,
}

/// The most of a custom agent's prompt `ListCustomAgents` gives.
pub const MAX_CUSTOM_AGENT_PROMPT: usize = 16 * 1024;

/// The most custom agents one `ListCustomAgents` gives.
pub const MAX_CUSTOM_AGENTS: usize = 200;

wire_enum! {
    /// The coding agent harness that defines a `CustomAgent`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum CustomAgentHarness {
        0 Claude {}
        1 OpenCode {}
        2 Codex {}
        3 Pi {}
        4 Flsh {}
    }
}

impl CustomAgentHarness {
    /// The harness's id, as the apps name it elsewhere: `claude`,
    /// `opencode`, `codex`, `pi` or `flsh`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claude {} => "claude",
            Self::OpenCode {} => "opencode",
            Self::Codex {} => "codex",
            Self::Pi {} => "pi",
            Self::Flsh {} => "flsh",
        }
    }
}

wire_enum! {
    /// Where a `CustomAgent` is defined: `Project` in the project folder,
    /// so it travels with the repository, `User` in the user's own harness
    /// folder on the machine.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum CustomAgentScope {
        0 Project {}
        1 User {}
    }
}

impl CustomAgentScope {
    /// `project` or `user`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Project {} => "project",
            Self::User {} => "user",
        }
    }
}

/// What `GetProjectLists` gives.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectLists {
    pub pull_requests: Vec<ProjectPullRequest>,
    /// Why the pull requests could not be read; the list is then the one
    /// read before, if any.
    pub prs_error: Option<String>,
    /// The open issues no task tracks, newest first.
    pub issues: Vec<ProjectIssue>,
    /// As `prs_error`, for the issues.
    pub issues_error: Option<String>,
    /// When the host was last read; `None` before the first read and for a
    /// project without a GitHub or Forgejo host.
    pub synced_at: Option<SystemTime>,
}

/// What `ListProjectRuns` gives.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectRuns {
    /// Newest first.
    pub runs: Vec<WorkflowRun>,
    /// How the newest runs of each open pull request stand.
    pub checks: Vec<PrChecks>,
    /// When the run list was last read; `None` before the first read.
    pub synced_at: Option<SystemTime>,
    /// Why the runs could not be read, for the user, such as a host on the
    /// user's own network or no token for it. The list is the one read
    /// before, if any.
    pub error: Option<String>,
    /// The host's rate limit for the caller's token runs low: live
    /// progress waits until then.
    pub rate_limited_until: Option<SystemTime>,
}

/// One workflow run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowRun {
    /// The host's id, which `GetProjectRun` takes.
    pub id: u64,
    /// Its number in the repository.
    pub number: u64,
    /// The workflow's name, or its file where the host gives no name.
    pub workflow: String,
    /// The commit or pull request title it ran for.
    pub title: String,
    pub branch: String,
    /// `push`, `pull_request`, `schedule` and so on, as the host says.
    pub event: String,
    /// Who triggered it; empty when the host does not say.
    pub actor: String,
    pub state: RunState,
    /// Its web page.
    pub url: String,
    pub started_at: Option<SystemTime>,
    /// `None` while it runs.
    pub finished_at: Option<SystemTime>,
    /// How far it got, for a run in progress whose jobs were read.
    pub progress: Option<RunProgress>,
    /// The pull requests it ran for, where the host says.
    pub pull_requests: Vec<u64>,
}

wire_enum! {
    /// How a run, job or step stands. `Waiting` waits for an approval or
    /// a runner; `Unknown` is a state the host names that Beep does not
    /// know.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::IntoStaticStr)]
    #[strum(serialize_all = "snake_case")]
    pub enum RunState {
        0 Queued {}
        1 Waiting {}
        2 Running {}
        3 Passed {}
        4 Failed {}
        5 Cancelled {}
        6 Skipped {}
        7 Unknown {}
    }
}

impl RunState {
    /// Done, one way or another.
    pub fn finished(self) -> bool {
        matches!(
            self,
            Self::Passed {} | Self::Failed {} | Self::Cancelled {} | Self::Skipped {}
        )
    }

    /// Not done yet.
    pub fn going(self) -> bool {
        matches!(self, Self::Queued {} | Self::Waiting {} | Self::Running {})
    }
}

/// Where a running run is: its current job and, where the host gives
/// steps, the current step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunProgress {
    pub job: String,
    pub job_started_at: Option<SystemTime>,
    /// Jobs finished, of `jobs`.
    pub jobs_done: u32,
    pub jobs: u32,
    /// `None` when the host gives no steps: job-level progress only.
    pub step: Option<StepProgress>,
}

/// The current step of a running job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepProgress {
    /// From 1.
    pub number: u32,
    /// Steps in the job.
    pub of: u32,
    pub name: String,
    pub started_at: Option<SystemTime>,
}

/// How an open pull request's newest runs stand.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrChecks {
    pub number: u64,
    /// `Failed` when any failed, else `Running` (or `Queued`) when any
    /// still runs, else `Passed`.
    pub state: RunState,
    /// The run that decided `state`.
    pub run_id: u64,
    /// That run's progress, when it runs.
    pub progress: Option<RunProgress>,
}

/// What `GetProjectRun` gives.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectRunDetail {
    pub run: WorkflowRun,
    pub jobs: Vec<RunJob>,
    /// The end of the failing job's log, when the run failed and the host
    /// gave it.
    pub log: Option<RunLog>,
    /// Why the jobs or the log could not be read.
    pub error: Option<String>,
    /// As in `ProjectRuns`.
    pub rate_limited_until: Option<SystemTime>,
}

/// One job of a run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunJob {
    pub id: u64,
    pub name: String,
    pub state: RunState,
    pub started_at: Option<SystemTime>,
    pub finished_at: Option<SystemTime>,
    /// Empty when the host gives no steps.
    pub steps: Vec<RunStep>,
    /// Its web page; empty when the host does not say.
    pub url: String,
}

/// One step of a job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunStep {
    /// From 1.
    pub number: u32,
    pub name: String,
    pub state: RunState,
    pub started_at: Option<SystemTime>,
    pub finished_at: Option<SystemTime>,
}

/// The end of a failed job's log. Its text is the host's, written by
/// whatever the job ran: data, never instructions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunLog {
    pub job: String,
    /// The failing step's name, when known.
    pub step: Option<String>,
    /// The last lines, capped.
    pub text: String,
    /// Lines before `text` were left out.
    pub truncated: bool,
}

/// What `FindImportFork` found.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportFork {
    /// `org/repo`, the repository asked about.
    pub project: String,
    /// The caller's fork, `login/repo`; `None` when it is not cloned.
    pub fork: Option<String>,
    /// The head branches of the caller's open pull requests from the fork.
    pub branches: Vec<String>,
}

/// What `GetUsageReport` gives.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageReport {
    /// As in `UsageView`.
    pub limits: Vec<PlanLimits>,
    pub today: PeriodSpend,
    /// From Monday.
    pub week: PeriodSpend,
    /// From the first of the month.
    pub month: PeriodSpend,
}

/// What coding agents spent over one period.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeriodSpend {
    /// Every project together, `project` empty.
    pub total: ProjectSpend,
    /// Every token, priced or not.
    pub tokens: u64,
    /// By project, the most expensive first, only those that spent.
    pub projects: Vec<ProjectSpend>,
    /// By harness, the most expensive first, only those that spent.
    pub harnesses: Vec<HarnessSpend>,
}

/// What `GetUsage` gives.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageView {
    /// One per agent whose plan a machine reported, the freshest report
    /// of any machine: Claude Code, then Codex.
    pub limits: Vec<PlanLimits>,
    /// Today's spending in the caller's timezone, by project, the most
    /// expensive first.
    pub today: Vec<ProjectSpend>,
    /// Today's spending by harness, only those that spent anything.
    pub harnesses: Vec<HarnessSpend>,
    /// The whole week so far, from Monday.
    pub week: ProjectSpend,
}

/// How much of one coding agent's plan is used.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanLimits {
    /// `claude` or `codex`.
    pub agent: AgentSlug,
    /// The plan, such as `max` or `pro`, when the agent says: free-form
    /// text the agent reports.
    pub plan: Option<String>,
    /// The machine that read it.
    pub machine_name: String,
    /// When it was read.
    pub as_of: SystemTime,
    /// The shortest window first.
    pub windows: Vec<PlanWindow>,
}

/// One rate limit window of a plan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanWindow {
    /// How long the window is: 300 for five hours, 10080 for a week.
    pub minutes: u32,
    /// How much of it is used, from 0 to 100.
    pub used_percent: f64,
    pub resets_at: Option<SystemTime>,
    /// What it is limited to, such as `Opus`; `None` for the whole plan.
    /// Free-form text the agent reports.
    pub scope: Option<String>,
}

/// What coding agents spent in one project, or in all of them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectSpend {
    /// `org/repo`, else the folder's name; empty for a total.
    pub project: String,
    /// At API prices, in US dollars: an estimate on a flat-rate plan.
    pub cost_usd: f64,
    /// The tokens of the agents that report no cost, such as Codex.
    pub unpriced: Vec<AgentTokens>,
}

/// The tokens one agent used.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentTokens {
    pub agent: AgentSlug,
    pub tokens: u64,
}

/// What one coding agent harness spent today.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HarnessSpend {
    /// Its slug, such as `claude`.
    pub agent: AgentSlug,
    /// At API prices, in US dollars, when it reports a cost; `None` for
    /// one that reports tokens only, such as Codex.
    pub cost_usd: Option<f64>,
    /// Every token its turns used.
    pub tokens: u64,
    /// The tokens by model, the most first, when it says.
    pub models: Vec<ModelSpend>,
}

/// The tokens one model used.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelSpend {
    pub model: ModelId,
    pub tokens: u64,
}

/// What `GetSpendReport` gives.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpendReport {
    /// As in `UsageView`.
    pub limits: Vec<PlanLimits>,
    pub today: SpendPeriod,
    /// From Monday.
    pub week: SpendPeriod,
    /// From the first of the month.
    pub month: SpendPeriod,
}

/// What coding agents spent over one period, metered use apart from use a
/// subscription covered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpendPeriod {
    /// Metered API spend in US dollars, every project together.
    pub cost_usd: f64,
    /// The tokens of metered use, priced or not.
    pub metered_tokens: u64,
    /// The tokens a subscription covered.
    pub subscription_tokens: u64,
    /// By project, the most expensive first, then the most tokens, only
    /// those that spent.
    pub projects: Vec<ProjectBill>,
    /// By harness and billing, the most expensive first, then the most
    /// tokens, only those that spent.
    pub harnesses: Vec<HarnessBill>,
}

/// What coding agents spent in one project.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectBill {
    /// `org/repo`, else the folder's name.
    pub project: String,
    /// Metered API spend in US dollars.
    pub cost_usd: f64,
    /// The tokens of metered use, priced or not.
    pub metered_tokens: u64,
    /// The tokens a subscription covered, by agent.
    pub subscription: Vec<AgentTokens>,
}

/// What one harness spent one way: metered, or on a subscription.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HarnessBill {
    /// Its slug, such as `claude`.
    pub agent: AgentSlug,
    /// A subscription covered it: tokens only, `cost_usd` is `None`.
    pub subscription: bool,
    /// Metered API spend in US dollars; `None` when none of it was priced
    /// or a subscription covered it.
    pub cost_usd: Option<f64>,
    pub tokens: u64,
    /// The tokens by model, the most first, when it says.
    pub models: Vec<ModelSpend>,
}

/// The coding agent harnesses on one machine, as `ListAgentHarnesses`
/// gives them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentHarnesses {
    pub machine_id: String,
    pub machine_name: String,
    /// When the machine last looked the newest versions up; `None` when it
    /// never did.
    pub checked_at: Option<SystemTime>,
    /// Every harness Beep knows, installed or not, then any other the
    /// machine runs.
    pub harnesses: Vec<AgentHarness>,
}

/// One harness on a machine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentHarness {
    /// Its slug, such as `claude`.
    pub agent: String,
    /// Its CLI is on the machine.
    pub installed: bool,
    /// The machine can start it; only these are offered.
    pub runnable: bool,
    /// Its version, such as `2.1.312`.
    pub version: Option<String>,
    /// The newest version, when it could be looked up.
    pub latest: Option<String>,
    /// What the machine starts for it when no binary path is set.
    pub program: Option<String>,
    /// The variable its home folder is set with, such as `CODEX_HOME`.
    pub home_var: Option<String>,
    /// Its home folder when that variable is not set, such as `~/.codex`.
    pub home_default: Option<String>,
    pub sign_in: HarnessSignIn,
    /// The command Install or Update runs there; `None` when there is none.
    pub install: Option<String>,
    pub settings: HarnessSettings,
}

wire_enum! {
    /// How a harness signs in on a machine. `BeepToken` is the token Beep
    /// keeps for it (Agent accounts), handed to every session it starts.
    #[derive(Debug, Clone, PartialEq, Eq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum HarnessSignIn {
        0 Unknown {}
        1 SignedIn { account: Option<String>, plan: Option<String> }
        2 SignedOut {}
        3 BeepToken { last4: String }
    }
}

/// The caller's settings for one harness on one machine. Empty text means
/// what the machine finds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HarnessSettings {
    /// Off: not offered on that machine.
    pub enabled: bool,
    /// The program to start in place of the one the machine finds.
    pub program: String,
    /// Its home folder, set with its home variable.
    pub home: String,
    /// Added to every session it starts.
    pub args: Vec<String>,
    pub env: Vec<HarnessEnv>,
}

/// An env var a harness gets. The server never sends a value back: in
/// replies `value` is always `None`. Not for the phone app, which has no
/// `SecretText` type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HarnessEnv {
    pub name: String,
    pub value: Option<SecretText>,
}

/// One project's sync in detail, as `GetProjectSyncDetail` gives it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectSyncDetail {
    pub project: String,
    pub pinned: bool,
    /// Paused on every machine with `PauseProjectSync`.
    pub paused: bool,
    pub machines: Vec<MachineSyncDetail>,
}

/// One machine's sync of a project in detail.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MachineSyncDetail {
    /// How it stands, as `ListProjectSync` gives it. While paused its
    /// status is `InSync`, or `Offline` when it does not answer.
    pub sync: MachineSync,
    /// The machine reports itself paused.
    pub paused: bool,
    /// What it is bringing in line now, while it syncs.
    pub progress: Option<SyncProgress>,
    /// Some paths it still has to bring, or that failed and are tried
    /// again, at most 50.
    pub waiting: Vec<String>,
    /// Its beep-node can settle conflicts with `ResolveSyncConflict`.
    pub can_resolve: bool,
}

/// How far a machine is through what it brings in line now.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncProgress {
    pub files_done: u64,
    pub files_total: u64,
    pub bytes_done: u64,
    pub bytes_total: u64,
    /// Bytes a second over the last few seconds; 0 before it can tell.
    pub rate: u64,
    /// Seconds left at that rate; `None` before it can tell.
    pub eta_secs: Option<u32>,
}

wire_enum! {
    /// Which side of a conflict `ResolveSyncConflict` keeps: the file the
    /// copy stands beside (`Original`), or the conflict copy (`Copy`).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum ConflictKeep {
        0 Original {}
        1 Copy {}
    }
}

/// A machine whose folder of a project Syncthing keeps in step, as
/// `ListProjectSyncthing` gives it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncthingPlace {
    /// The `Machine::id` it is.
    pub machine_id: String,
    /// The project's folder there.
    pub folder: String,
    /// The Syncthing folder's label there, or its ID without one.
    pub label: String,
}

/// Folders the apps list projects under, as `ListProjectGroups` gives
/// them. A project is in a group when its folder on any machine is one of
/// `paths` or inside one, both compared as `groups::project_group_path`
/// gives them.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectGroup {
    pub id: Uuid,
    /// What the group is called: its first folder until renamed.
    pub name: String,
    /// The folders, such as `work`, oldest first.
    pub paths: Vec<String>,
}

/// How the user's coding agents sign in, as `GetAgentAccounts` gives it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentAccounts {
    /// The Claude Code token the machines get, when there is one.
    pub claude: Option<AgentToken>,
    /// The API key Codex uses instead of each machine's own sign-in.
    pub codex_key: Option<AgentToken>,
    /// The caller's machines with Claude Code or Codex, by name.
    pub machines: Vec<AgentMachine>,
}

/// A token Beep keeps for a coding agent. The secret stays on the server.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentToken {
    /// The secret's last four characters.
    pub last4: String,
    pub created_at: SystemTime,
    /// The server can no longer open it; the user has to sign in again.
    pub unreadable: bool,
}

/// One of the user's machines with Claude Code or Codex.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentMachine {
    pub machine_id: String,
    pub machine_name: String,
    pub online: bool,
    /// Claude Code is on it, as it last said.
    pub claude: bool,
    /// Codex is on it, as it last said.
    pub codex: bool,
    /// Claude Code sessions Beep runs on it now.
    pub claude_sessions: u32,
    /// Whether Codex is signed in there; `None` when not known, such as
    /// while it is offline.
    pub codex_signed_in: Option<bool>,
}

/// A Codex device sign-in under way on a machine.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodexLogin {
    /// The node's background task running `codex login`.
    pub task: u32,
    /// The page to enter the code on.
    pub url: String,
    pub code: String,
    /// When the code stops working.
    pub expires_at: SystemTime,
}

wire_enum! {
    /// How a Codex device sign-in stands. `Gone` is no longer on its
    /// machine, such as after a restart, or the machine is offline.
    #[derive(Debug, Clone, PartialEq, Eq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum CodexLoginState {
        0 Waiting {}
        1 SignedIn {}
        2 Failed { message: String }
        3 Gone {}
    }
}

/// The coding agents Beep can keep a token for. On the wire an agent is
/// its slug, the name the node gives the agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::IntoStaticStr, strum::EnumString)]
#[strum(serialize_all = "lowercase")]
pub enum AgentAccount {
    Claude,
    Codex,
}

impl AgentAccount {
    /// The slug the wire and the database carry, such as `claude`.
    pub fn slug(self) -> &'static str {
        self.into()
    }

    /// The env var the agent reads its token from: Claude Code's
    /// long-lived OAuth token, or the API key Codex takes in place of a
    /// sign-in.
    pub fn env_var(self) -> &'static str {
        match self {
            Self::Claude => "CLAUDE_CODE_OAUTH_TOKEN",
            Self::Codex => "CODEX_API_KEY",
        }
    }
}

/// The models a coding agent can run on a machine, as `ListAgentModels`
/// gives them.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentModels {
    pub pick: AgentModelPick,
    /// The model the agent runs when none is picked, when it says.
    pub current: Option<String>,
    /// The models to pick from, for `AgentModelPick::Listed`.
    pub models: Vec<AgentModel>,
}

wire_enum! {
    /// How the start form picks a coding agent's model: `Fixed` when none
    /// can be picked (the agent runs its own, or the machine's beep-node is
    /// too old to pick one), `Listed` from `AgentModels::models`, `Named`
    /// by any name the agent takes when it lists none.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr)]
    #[strum(serialize_all = "snake_case")]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum AgentModelPick {
        0 Fixed {}
        1 Listed {}
        2 Named {}
    }
}

wire_enum! {
    /// What a coding agent starts on, as `StartAgentSession::checkout`
    /// asks: a new `beep/<slug>` branch in the checkout itself, or one in a
    /// new worktree at `.worktrees/beep-<slug>`, made from `base` (the
    /// checkout's `HEAD` when `None`). `NamedWorktree` is `NewWorktree` on
    /// a branch named `branch` rather than from the prompt.
    /// `BranchWorktree` starts on the existing branch `branch`: in the
    /// worktree that has it checked out, else in a new one at
    /// `.worktrees/<branch>` after fetching it from `origin`. A server
    /// older than `Feature::BRANCH_WORKTREES` refuses both, and so does a
    /// node from before them. See `node::agent::CheckoutPlan`.
    #[derive(Debug, Clone, PartialEq, Eq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum AgentCheckoutPlan {
        0 NewBranch { base: Option<String> }
        1 NewWorktree { base: Option<String> }
        2 NamedWorktree { base: Option<String>, branch: String } since BRANCH_WORKTREES
        3 BranchWorktree { branch: String } since BRANCH_WORKTREES
    }
}

wire_enum! {
    /// How hard a coding agent thinks, as `StartAgentSession::effort`
    /// asks. Which levels an agent takes is `node::agent::efforts`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr)]
    #[strum(serialize_all = "snake_case")]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum AgentEffort {
        0 Off {}
        1 Minimal {}
        2 Low {}
        3 Medium {}
        4 High {}
        5 Xhigh {}
        6 Max {}
    }
}

/// One model a coding agent offers.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentModel {
    /// What `StartAgentSession::model` names it by.
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

/// The machine Auto would start a coding agent on, as
/// `PreviewAgentMachine` gives it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MachinePick {
    pub machine_id: String,
    pub machine_name: String,
    /// Why, such as "It has Xcode and runs the fewest sessions."
    pub why: String,
}

/// A project's branches on one machine, as `ListProjectBranches` reads them.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectBranches {
    pub machine_id: String,
    pub machine_name: String,
    pub default_branch: String,
    pub branches: Vec<ProjectBranch>,
}

/// One branch of a project.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectBranch {
    /// Without `origin/`.
    pub name: String,
    /// The folder of the worktree that has it checked out, on the machine
    /// read.
    pub worktree: Option<String>,
    /// Commits it has that the default branch does not.
    pub ahead: u32,
    /// Commits the default branch has that it does not.
    pub behind: u32,
    /// Its last commit.
    pub head: String,
    pub subject: String,
    pub committed_at: SystemTime,
    /// It is a branch of the checkout, not only on `origin`.
    pub local: bool,
    /// It is on `origin`.
    pub pushed: bool,
}

/// Where a reviewed branch is now, as `CheckBranchReview` reads it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewHead {
    /// The local branch's head, else `origin`'s; `None` when the branch is
    /// gone.
    pub head: Option<String>,
    /// It moved since the review, so its findings may be out of date.
    pub stale: bool,
}

/// A branch to give a worktree, and where.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BranchPlace {
    pub branch: String,
    /// The machine; `None` for the one the project is imported on.
    pub machine_id: Option<String>,
}

/// What `PreviewImport` found for a repository.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportPreview {
    /// `org/repo`.
    pub project: String,
    /// The checkouts found, the one the branches were read in first.
    pub checkouts: Vec<ImportCheckout>,
    /// Empty when no checkout was read.
    pub default_branch: String,
    pub branches: Vec<WorkBranch>,
    /// What could not be read, such as the pull requests; empty when all
    /// was.
    pub note: String,
}

/// A checkout of a repository on one of the caller's machines.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportCheckout {
    pub machine_id: String,
    pub machine_name: String,
    pub folder: String,
    /// The remote that is the repository, such as `origin` or `upstream`.
    pub remote: String,
}

/// A branch with work, as `PreviewImport` lists it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkBranch {
    pub name: String,
    /// The caller's open pull request from it.
    pub pr: Option<u64>,
    pub pr_title: String,
    /// Commits ahead of the default branch; 0 when not read.
    pub ahead: u32,
    /// Its worktree's folder, on the first checkout's machine.
    pub worktree: Option<String>,
    /// It is in the checkout read, locally or on `origin`.
    pub in_checkout: bool,
    /// Its last commit; `None` when not read.
    pub committed_at: Option<SystemTime>,
}

/// A review of a branch against its base, as it stands.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BranchReview {
    pub id: Uuid,
    /// `org/repo`.
    pub project: String,
    pub branch: String,
    /// The branch it merges into.
    pub base: String,
    /// The commit reviewed.
    pub head: String,
    pub head_at: SystemTime,
    pub started_at: SystemTime,
    /// The machine whose checkout the reviewers read.
    pub machine_name: String,
    /// Lines added and removed.
    pub changed_lines: u32,
    pub files: u32,
    pub merge: ReviewMerge,
    /// The branch's open pull request, when it has one.
    pub pr: Option<ReviewPr>,
    pub shards: Vec<ReviewShard>,
    /// Ranked: open ones by severity first, dismissed then dropped ones
    /// last.
    pub findings: Vec<ReviewFinding>,
}

/// A reviewed branch's pull request.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewPr {
    pub number: u64,
    pub url: String,
    pub checks: ReviewChecks,
}

wire_enum! {
    /// How the checks on a pull request stand. `None` when it reports
    /// none or the host does not say.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr)]
    #[strum(serialize_all = "snake_case")]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum ReviewChecks {
        0 None {}
        1 Pending {}
        2 Passing {}
        3 Failing {}
    }
}

wire_enum! {
    /// Whether the branch merges into its base. `Unknown` when git on the
    /// machine is too old to say.
    #[derive(Debug, Clone, PartialEq, Eq, strum::IntoStaticStr)]
    #[strum(serialize_all = "snake_case")]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum ReviewMerge {
        0 Clean {}
        1 Conflicts { files: Vec<String> }
        2 Unknown {}
    }
}

/// One reviewer's part of the diff.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewShard {
    /// The folder its files are under, such as `apps/web`, or `.` for the
    /// files at the top.
    pub area: String,
    pub files: u32,
    pub lines: u32,
    /// The coding agent reading it, such as `claude`.
    pub agent: String,
    pub state: ReviewShardState,
    /// Findings its reviewer reported.
    pub found: u32,
}

wire_enum! {
    /// Where one shard stands. `Reading` says what the reviewer does now;
    /// `Verifying` waits for the verifier.
    #[derive(Debug, Clone, PartialEq, Eq, strum::IntoStaticStr)]
    #[strum(serialize_all = "snake_case")]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum ReviewShardState {
        0 Reading { doing: String }
        1 Verifying {}
        2 Done {}
        3 Failed { why: String }
    }
}

/// One thing a reviewer found.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewFinding {
    /// Unique within its review.
    pub id: u32,
    /// Its shard's area.
    pub area: String,
    pub severity: ReviewSeverity,
    pub title: String,
    pub file: String,
    pub line: Option<u32>,
    /// One line on why it matters.
    pub why: String,
    pub verdict: ReviewVerdict,
    pub dismissed: bool,
}

wire_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, strum::IntoStaticStr)]
    #[strum(serialize_all = "snake_case")]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum ReviewSeverity {
        0 High {}
        1 Medium {}
        2 Low {}
    }
}

wire_enum! {
    /// What the verifier made of a finding. `Unchecked` when it could not
    /// say, so the finding stays.
    #[derive(Debug, Clone, PartialEq, Eq, strum::IntoStaticStr)]
    #[strum(serialize_all = "snake_case")]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum ReviewVerdict {
        0 Waiting {}
        1 Verified {}
        2 Dropped { why: String }
        3 Unchecked {}
    }
}

/// A command kept running on one of the user's machines, for a project or
/// on its own.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Terminal {
    pub id: Uuid,
    /// `org/repo`, or empty for a terminal of no project.
    pub project: String,
    pub machine_id: String,
    pub machine_name: String,
    pub name: String,
    pub command: String,
    /// The folder it runs in.
    pub cwd: String,
    pub state: TerminalState,
    pub started_at: SystemTime,
}

wire_enum! {
    /// How a terminal stands. `Exited` ended on its own (`code` is `None`
    /// when a signal ended it); `Offline` is not known while its machine
    /// is offline; `Gone` is no longer on its machine, such as after a
    /// restart.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr)]
    #[strum(serialize_all = "snake_case")]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum TerminalState {
        0 Running {}
        1 Exited { code: Option<i32> }
        2 Stopped {}
        3 Offline {}
        4 Gone {}
    }
}

/// A terminal and the last of its output, as `GetTerminal` gives it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TerminalOutput {
    pub terminal: Terminal,
    /// Oldest first.
    pub lines: Vec<String>,
    /// Lines were dropped before these.
    pub earlier: bool,
}

impl Duplex for AttachTerminal {
    type Up = TerminalInput;
}

wire_enum! {
    /// What `AttachTerminal` streams. `Output` is the terminal's output
    /// from byte `at`, raw, escape sequences and all; an `at` past the
    /// asked `from` means the bytes in between were no longer kept.
    /// `Ended` says how it stood when the stream ended: it stopped, or its
    /// machine went offline. `ViewOnly` is a terminal that takes no input,
    /// to read with `GetTerminal` instead.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum TerminalEvent {
        0 Output { at: u64, data: serde_bytes::ByteBuf }
        1 Ended { state: TerminalState }
        2 ViewOnly {}
    }
}

wire_enum! {
    /// What the client sends after `AttachTerminal`: keys typed, at most
    /// `MAX_TERMINAL_INPUT` bytes at a time, or the terminal's new size.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum TerminalInput {
        0 Keys { data: serde_bytes::ByteBuf }
        1 Resize { cols: u32, rows: u32 }
    }
}

/// The most one `TerminalInput::Keys` may carry.
pub const MAX_TERMINAL_INPUT: usize = node::terminal::MAX_INPUT;

#[cfg(test)]
impl compat::Sample for serde_bytes::ByteBuf {
    fn samples(_: u32) -> Vec<Self> {
        vec![serde_bytes::ByteBuf::from(
            b"$ ls\r\n\x1b[32mok\x1b[0m".to_vec(),
        )]
    }
}

/// One saved version of the user's instructions.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstructionsVersion {
    /// 1 for the first save, then one more each time.
    pub version: u32,
    /// Markdown.
    pub text: String,
    /// What made it, such as "Restored version 3"; empty for an edit.
    pub note: String,
    pub saved_at: SystemTime,
}

/// The agent files on one machine, as `ReadAgentFiles` reads them.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MachineAgentFiles {
    pub machine_id: String,
    pub machine_name: String,
    pub files: Vec<AgentFile>,
    /// Why the machine could not be read or written; `files` is then empty.
    pub error: Option<String>,
}

/// One coding agent's instructions file on a machine.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentFile {
    /// `claude`, `codex`, `gemini` or `pi`.
    pub agent: String,
    /// Under the home folder, such as `~/.claude/CLAUDE.md`.
    pub path: String,
    /// Its text; `None` when the agent's folder is there but the file is
    /// not.
    pub text: Option<String>,
    /// It holds the newest instructions between Beep's markers.
    pub synced: bool,
}

/// One project as `ListProjectOverview` sums it up.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectOverview {
    pub project_id: Uuid,
    /// `org/repo`.
    pub name: String,
    /// How many of its tasks stand in each state, dropped ones left out.
    pub tasks: TaskCounts,
    /// Its workers that have not ended.
    pub workers: u32,
    /// The task that changed last, dropped ones left out.
    pub latest: Option<ProjectTask>,
}

/// How many of a project's tasks stand in each state.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskCounts {
    pub planned: u32,
    pub running: u32,
    pub blocked: u32,
    pub review: u32,
    pub done: u32,
}

/// One of the caller's GitHub accounts, as `ListGitHubAccounts` lists it.
/// The token stays on the server.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GitHubAccount {
    /// The same id as its `Credential::id`.
    pub id: String,
    /// The login GitHub said the token signs in as.
    pub login: String,
    /// The orgs the token could see when it was saved.
    pub orgs: Vec<String>,
    /// The machines get it as `GH_TOKEN`, and projects no other account
    /// owns use it.
    pub default: bool,
    /// The token's last four characters.
    pub last4: String,
    pub created_at: SystemTime,
    /// The server can no longer open it; the user has to enter it again.
    pub unreadable: bool,
}

/// What `ListGitItems` found.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GitItems {
    pub items: Vec<GitItem>,
    /// Every git host in the caller's credentials, whether it answered or
    /// not. Empty when the caller has none.
    pub hosts: Vec<GitHost>,
}

/// One git host `ListGitItems` asked.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GitHost {
    /// `github.com`, or a Forgejo server's host, such as `git.example.com`.
    pub host: String,
    pub kind: GitHostKind,
    /// Who the token signs in as, when known.
    pub account: Option<String>,
    /// Why its items are missing, for the user; `None` when it answered.
    pub error: Option<String>,
}

wire_enum! {
    /// What kind of server a git host is.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum GitHostKind {
        0 GitHub {}
        1 Forgejo {}
    }
}

/// An open pull request or issue, as `ListGitItems` lists it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GitItem {
    /// As `GitHost::host`.
    pub host: String,
    /// `org/repo`.
    pub repo: String,
    pub number: u64,
    pub kind: GitItemKind,
    pub title: String,
    /// Its web page.
    pub url: String,
    pub updated_at: SystemTime,
    /// The caller opened it.
    pub authored: bool,
    /// The caller is assigned to it.
    pub assigned: bool,
    /// A pull request that asks for the caller's review.
    pub review_requested: bool,
    /// A draft pull request.
    pub draft: bool,
    /// How a pull request's reviews stand, where the host says.
    pub review: GitReview,
    /// A pull request whose checks fail on its head commit, where the host
    /// says.
    pub checks_failing: bool,
    pub comments: u32,
}

wire_enum! {
    /// Whether an item is a pull request or an issue.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::IntoStaticStr)]
    #[strum(serialize_all = "snake_case")]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum GitItemKind {
        0 PullRequest {}
        1 Issue {}
    }
}

wire_enum! {
    /// How a pull request's reviews stand. `None` for an issue, a PR with
    /// no deciding review yet, or a host that does not say.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::IntoStaticStr)]
    #[strum(serialize_all = "snake_case")]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum GitReview {
        0 None {}
        1 Approved {}
        2 ChangesRequested {}
    }
}

/// A project imported onto one of the caller's machines.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportedProject {
    /// `org/repo`.
    pub project: String,
    /// The repository's web page, such as `https://github.com/org/repo`.
    pub url: String,
    /// The `Machine::id` it is on.
    pub machine_id: String,
    pub machine_name: String,
    /// Its absolute folder on that machine.
    pub folder: String,
    /// This import cloned it, rather than finding it already there.
    pub cloned: bool,
    pub imported_at: SystemTime,
}

/// A folder on one of the caller's machines, as `ListMachineFolder` reads it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MachineFolder {
    /// Its absolute path on the machine.
    pub path: String,
    /// The folder above it; `None` at the root.
    pub parent: Option<String>,
    /// `false` for a folder that is not there yet; an import makes it.
    pub exists: bool,
    /// The folders in it by name, sorted, hidden ones left out.
    pub folders: Vec<String>,
    /// Folders left out past the limit.
    pub more: u32,
    /// It holds nothing, hidden files included.
    pub empty: bool,
    /// It is a git checkout: its `origin` remote, empty when it has none.
    pub origin: Option<String>,
}

/// A git checkout the caller's coding agents worked in, as
/// `ListFoundFolders` lists it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FoundProject {
    /// `org/repo` from its remote, else the remote as it is.
    pub project: String,
    /// Its `origin` remote, what `ImportProject::repo` takes.
    pub remote: String,
    /// Where it is, one folder per machine.
    pub places: Vec<FoundPlace>,
    /// Sessions per agent, on all of its machines, most first.
    pub agents: Vec<FoundAgent>,
    pub last_activity: SystemTime,
}

/// One machine's checkout of a `FoundProject`.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FoundPlace {
    pub machine_id: String,
    pub machine_name: String,
    /// Its absolute folder on that machine.
    pub folder: String,
}

/// One of the caller's machines as a project's page lists it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectMachine {
    pub machine_id: String,
    pub machine_name: String,
    pub online: bool,
    /// The project's absolute folder there; `None` without a checkout.
    pub folder: Option<String>,
    /// Beep is looking through it for a checkout now.
    pub checking: bool,
    /// When Beep last looked through it, since the project was imported.
    pub checked_at: Option<SystemTime>,
}

/// How many sessions of one agent ran in a found folder.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FoundAgent {
    /// Such as `claude`, `codex`, `pi`, `flsh` or `opencode`.
    pub agent: String,
    pub sessions: u32,
}

/// One project's sync across the caller's machines.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectSync {
    pub project: String,
    pub pinned: bool,
    pub machines: Vec<MachineSync>,
}

/// How one machine stands with a project it syncs.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MachineSync {
    /// The `Machine::id` it is.
    pub machine_id: String,
    pub machine_name: String,
    pub folder: String,
    pub status: SyncStatus,
    /// Files kept in step, and their bytes.
    pub files: u64,
    pub bytes: u64,
    /// Files another machine changed that this one does not have yet.
    pub pending: u64,
    /// The copies two machines' edits of one file left, by path in the
    /// folder, such as `notes (conflict 1a2b3c4d).txt`.
    pub conflicts: Vec<String>,
    /// Set when the machine rescans the folder this often instead of
    /// watching it, because its file watch limit is reached.
    pub rescan_secs: Option<u32>,
    /// What raises that limit, such as a `sysctl` line.
    pub hint: Option<String>,
    /// Why the machine's last scan or fetch failed.
    pub error: Option<String>,
}

wire_enum! {
    /// Where a machine's sync of a project stands: `Offline` (offline or
    /// did not answer), `Scanning` (looking for changes on disk),
    /// `Syncing` (fetching or writing what another machine changed),
    /// `InSync`, or `Failed` (`MachineSync::error` says why).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr)]
    #[strum(serialize_all = "snake_case")]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum SyncStatus {
        0 Offline {}
        1 Scanning {}
        2 Syncing {}
        3 InSync {}
        4 Failed {}
    }
}

/// What a project's sync leaves out on one machine.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncSkipped {
    pub paths: Vec<SkippedFile>,
    /// Files in a git repository that git carries between machines rather
    /// than sync.
    pub through_git: u64,
    /// More paths are left out than `paths` lists.
    pub truncated: bool,
}

/// A file or folder sync leaves out, and the rule that does.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkippedFile {
    /// Relative to the project folder, parts joined with `/`.
    pub path: String,
    pub folder: bool,
    pub rule: SkipRule,
}

wire_enum! {
    /// Why sync leaves a path out, in the order the rules apply: `Git`
    /// (`.git`, and in a git repository the files git carries), the
    /// project's `.gitignore`, a `Default` for the kind of project its
    /// `marker` file shows (such as `target/` for `Cargo.toml`), the
    /// project's `.beepignore`, or a symlink or special file (`Link`).
    #[derive(Debug, Clone, PartialEq, Eq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum SkipRule {
        0 Git {}
        1 GitIgnore {}
        2 Default { marker: String }
        3 BeepIgnore {}
        4 Link {}
    }
}

/// A credential as `ListCredentials` lists it. The secret stays on the
/// server.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Credential {
    pub id: String,
    pub kind: CredentialKind,
    /// The secret's last four characters.
    pub last4: String,
    /// Who the token signs in as, such as a GitHub login. `None` for API
    /// keys, and for a Forgejo server the server could not reach to check.
    pub account: Option<String>,
    /// The env var the machines get it as, such as `GH_TOKEN`. Empty for
    /// Forgejo, whose tokens go into teacli's config instead.
    pub env_var: String,
    /// Whether the caller's machines get it.
    pub to_machines: bool,
    pub created_at: SystemTime,
    /// The server can no longer open it; the user has to enter it again.
    /// Machines do not get it.
    pub unreadable: bool,
}

wire_enum! {
    /// What `CheckCredential` found. `machine` is the name of the machine
    /// that reached the server, `None` when Beep's server did. `message`
    /// says what happened, for the user.
    #[derive(Debug, Clone, PartialEq, Eq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum CredentialCheck {
        0 Valid { account: String, machine: Option<String> }
        1 Refused { machine: Option<String>, message: String }
        2 Unreachable { message: String }
    }
}

wire_enum! {
    /// What a credential is for. `url` is a Forgejo or Gitea server's base
    /// URL, such as `https://git.example.com`; `provider` an `ApiProvider`
    /// slug, such as `openrouter`.
    #[derive(Debug, Clone, PartialEq, Eq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum CredentialKind {
        0 GitHub {}
        1 Forgejo { url: String }
        2 ApiKey { provider: String }
    }
}

/// The providers an API key can be saved for. On the wire a provider is its
/// slug, so a newer server can add one without changing a reply's shape.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    strum::IntoStaticStr,
    strum::EnumString,
    strum::EnumIter,
)]
#[strum(serialize_all = "lowercase")]
pub enum ApiProvider {
    OpenRouter,
    ElevenLabs,
    OpenAi,
    Anthropic,
    Gemini,
    DeepSeek,
    Moonshot,
    Zai,
    /// Images and videos.
    Venice,
    /// 3D models.
    Meshy,
}

impl ApiProvider {
    /// Every provider, in the order Settings lists them.
    pub fn all() -> impl Iterator<Item = Self> {
        <Self as strum::IntoEnumIterator>::iter()
    }

    /// The slug the wire and the database carry, such as `openrouter`.
    pub fn slug(self) -> &'static str {
        self.into()
    }

    /// What the user knows it as.
    pub fn name(self) -> &'static str {
        match self {
            Self::OpenRouter => "OpenRouter",
            Self::ElevenLabs => "ElevenLabs",
            Self::OpenAi => "OpenAI",
            Self::Anthropic => "Anthropic",
            Self::Gemini => "Gemini",
            Self::DeepSeek => "DeepSeek",
            Self::Moonshot => "Moonshot",
            Self::Zai => "Z.ai",
            Self::Venice => "Venice",
            Self::Meshy => "Meshy",
        }
    }

    /// The env var tools read the key from.
    pub fn env_var(self) -> &'static str {
        match self {
            Self::OpenRouter => "OPENROUTER_API_KEY",
            Self::ElevenLabs => "ELEVENLABS_API_KEY",
            Self::OpenAi => "OPENAI_API_KEY",
            Self::Anthropic => "ANTHROPIC_API_KEY",
            Self::Gemini => "GEMINI_API_KEY",
            Self::DeepSeek => "DEEPSEEK_API_KEY",
            Self::Moonshot => "MOONSHOT_API_KEY",
            Self::Zai => "ZAI_API_KEY",
            Self::Venice => "VENICE_API_KEY",
            Self::Meshy => "MESHY_API_KEY",
        }
    }
}

/// The env var `gh` reads its token from.
pub const GITHUB_ENV_VAR: &str = "GH_TOKEN";

/// Something from one of the user's apps worth knowing, as `ListUpdates`
/// lists it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppUpdate {
    pub id: i64,
    /// One line, such as "Your TestFlight build is ready."
    pub summary: String,
    /// The app's slug, as `AppIntegration::slug` gives it, such as `gmail`.
    /// Empty when the events it came from are gone.
    pub app: String,
    /// How the user knows the app, such as "Gmail"; empty with `app`.
    pub app_name: String,
    /// When the triage agent decided to tell.
    pub at: SystemTime,
}

/// A setup token as `CreateNodeToken` made it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeToken {
    /// The token as text, such as `bk1-0f3ka-...`.
    pub token: String,
    pub expires_at: SystemTime,
}

/// A project and its agent chat, as `GetProjectAgent` and `ListProjects`
/// return it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectAgent {
    pub project_id: Uuid,
    /// `org/repo`.
    pub name: String,
    pub chat_id: Uuid,
    pub open_tasks: u32,
    pub open_workers: u32,
}

/// An open issue on a project's issues host, as `ListProjectIssues` gives it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectIssue {
    pub number: u64,
    pub title: String,
    /// Its label names.
    pub labels: Vec<String>,
    /// The login of who opened it.
    pub author: String,
    pub opened_at: SystemTime,
    pub comments: u32,
    /// Its web page.
    pub url: String,
}

/// An open pull request on a project's host, anyone's, as
/// `ListProjectPullRequests` gives it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectPullRequest {
    pub number: u64,
    pub title: String,
    /// Its web page.
    pub url: String,
    /// The login of who opened it.
    pub author: String,
    pub draft: bool,
    /// Opened by the account the host's CLI on the project's machine is
    /// signed in as; false when that account is unknown.
    pub mine: bool,
    pub created_at: SystemTime,
    pub updated_at: SystemTime,
}

/// The branch a pull request is made from, as `GetPullRequestHead` gives
/// it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PullRequestHead {
    pub number: u64,
    /// Its name, without `origin/`.
    pub branch: String,
    /// The branch is in another repository, a fork, so `origin` lacks it.
    pub fork: bool,
}

/// An open pull request or issue of the caller's, as
/// `ListProjectThreads` gives it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectThread {
    pub kind: GitItemKind,
    /// `org/repo`.
    pub repo: String,
    pub number: u64,
    pub title: String,
    /// Its web page.
    pub url: String,
    /// Who wrote last and the start of what they wrote, on one line, such
    /// as `sam: Can we cut it down?`; empty when nobody has.
    pub last_other: String,
    /// Someone other than the caller (bots aside) wrote after the caller's
    /// last comment or review.
    pub needs_reply: bool,
    pub updated_at: SystemTime,
}

/// An image sent to a coding agent, as `ListAgentImages` gives it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentImage {
    /// As the session's log names it.
    pub name: String,
    /// The file to download.
    pub file_id: String,
    pub mime: String,
    /// [`image_key`] of its bytes: an agent that replays the session
    /// gives the image back without its name, and the node logs it as
    /// `[image: image-<key>]`.
    pub key: String,
}

/// One picture of a coding agent session's log, as `GetAgentImage` gives it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentImageData {
    /// [`image_key`] of `data`.
    pub key: String,
    pub mime: String,
    #[serde(with = "serde_bytes")]
    pub data: Vec<u8>,
}

/// Part of a media file on a machine, as `GetAgentFile` gives it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentFileChunk {
    /// The whole file's size in bytes.
    pub size: u64,
    pub mime: String,
    /// The bytes from the asked offset, at most [`AGENT_FILE_CHUNK`]; empty
    /// at or past the end.
    #[serde(with = "serde_bytes")]
    pub data: Vec<u8>,
}

/// The most of a file one `GetAgentFile` gives, well under a frame.
pub const AGENT_FILE_CHUNK: usize = 4 * 1024 * 1024;

/// The largest file `GetAgentFile` reads.
pub const MAX_AGENT_FILE: u64 = 128 * 1024 * 1024;

/// The largest file `WriteWorkFile` writes.
pub const MAX_WORK_UPLOAD: u64 = 64 * 1024 * 1024;

/// The most entries one `ListWorkFolder` gives.
pub const MAX_WORK_ENTRIES: usize = 2000;

/// The most levels one `ListWorkTree` reads.
pub const MAX_TREE_DEPTH: u32 = 6;

/// About the most entries one `ListWorkTree` gives: it stops reading
/// folders past it.
pub const MAX_TREE_ENTRIES: usize = 5000;

/// Folders of a session's working folder, as `ListWorkTree` reads them.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkTree {
    /// The folder asked first, then those under it, each in full. A folder
    /// not here is not read yet.
    pub folders: Vec<WorkFolder>,
    /// It stopped at [`MAX_TREE_ENTRIES`] before `depth`.
    pub truncated: bool,
}

/// One folder of a session's working folder, as `ListWorkFolder` reads it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkFolder {
    /// The folder, relative to the root, `""` for the root.
    pub path: String,
    /// Folders first, then files, each by name.
    pub entries: Vec<WorkEntry>,
    /// Entries left out past [`MAX_WORK_ENTRIES`].
    pub more: u32,
    /// The folder is in a git checkout, so `ignored` and `change` mean
    /// something.
    pub git: bool,
}

/// A file or folder in a `WorkFolder`.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkEntry {
    pub name: String,
    pub folder: bool,
    /// A file's size in bytes; 0 for a folder.
    pub size: u64,
    /// Git ignores it.
    pub ignored: bool,
    /// What git sees changed in it, or anywhere under a folder.
    pub change: Option<GitChange>,
}

wire_enum! {
    /// How git sees a file changed, as `WorkEntry::change` marks it. A
    /// folder takes the strongest of what is under it, the last here first.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum GitChange {
        0 Untracked {}
        1 Added {}
        2 Modified {}
        3 Conflicted {}
    }
}

/// The most changed lines one file of a `WorkDiff` gives.
pub const MAX_DIFF_FILE_LINES: u32 = 2_000;

/// The most lines a `WorkDiff` gives in all.
pub const MAX_DIFF_LINES: u32 = 20_000;

/// The most lines a file named in `GetWorkDiff::only` gives.
pub const MAX_DIFF_SHOWN_LINES: u32 = 50_000;

/// The most files a `WorkDiff` lists; `WorkDiff::more` counts the rest.
pub const MAX_DIFF_FILES: usize = 1_000;

/// What changed in a checkout, as `GetWorkDiff` reads it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkDiff {
    /// The folder the diff was read in, absolute: a branch's worktree
    /// when it has one. File paths are relative to it.
    pub root: String,
    /// What it compares against, as git names it (`origin/main`), or
    /// `HEAD` in a checkout without a default branch.
    pub base: String,
    /// The commit the changes start from, where the head left the base;
    /// empty before the first commit.
    pub merge_base: String,
    /// Uncommitted changes and untracked files are in it.
    pub working_tree: bool,
    pub files: Vec<DiffFile>,
    /// Added and removed lines in every file, those without lines too.
    pub additions: u32,
    pub deletions: u32,
    /// Files left out past [`MAX_DIFF_FILES`].
    pub more: u32,
}

/// One file of a `WorkDiff`.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffFile {
    /// Relative to `WorkDiff::root`, `/` between names; the new path of a
    /// rename.
    pub path: String,
    /// Where a renamed file was.
    pub old_path: Option<String>,
    pub status: DiffStatus,
    pub additions: u32,
    pub deletions: u32,
    /// Empty for a binary file, and when `left_out` says why.
    pub hunks: Vec<DiffHunk>,
    pub left_out: Option<DiffLeftOut>,
}

/// One `@@` hunk of a `DiffFile`.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffHunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    /// What git shows after the second `@@`, such as the function.
    pub section: String,
    pub lines: Vec<DiffLine>,
}

/// One line of a hunk, without its marker or line ending.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub text: String,
    /// The file ends after this line without a newline.
    pub no_eol: bool,
}

wire_enum! {
    /// How a `DiffFile` changed.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum DiffStatus {
        0 Added {}
        1 Modified {}
        2 Deleted {}
        3 Renamed {}
        4 Binary {}
    }
}

wire_enum! {
    /// What a `DiffLine` is: unchanged context, added or removed.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum DiffLineKind {
        0 Context {}
        1 Added {}
        2 Removed {}
    }
}

wire_enum! {
    /// Why a `DiffFile` comes without its lines: past
    /// [`MAX_DIFF_FILE_LINES`], a lockfile asked without `lockfiles`, or
    /// past [`MAX_DIFF_LINES`] with the files before it.
    /// `GetWorkDiff::only` asks for them.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum DiffLeftOut {
        0 TooLarge {}
        1 Lockfile {}
        2 OverTotal {}
    }
}

/// The MIME type of a media file `GetAgentFile` reads, by the extension of
/// `path`, any case; `None` for any other file.
pub fn media_mime(path: &str) -> Option<&'static str> {
    let name = path.rsplit(['/', '\\']).next()?;
    let (stem, ext) = name.rsplit_once('.')?;
    if stem.is_empty() {
        return None;
    }
    Some(match ext.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "svg" => "image/svg+xml",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "glb" => "model/gltf-binary",
        "gltf" => "model/gltf+json",
        "obj" => "model/obj",
        "stl" => "model/stl",
        "pdf" => "application/pdf",
        _ => return None,
    })
}

/// A short name for an image's bytes, the same on the node and the server.
pub fn image_key(data: &[u8]) -> String {
    blake3::hash(data).to_hex()[..16].to_owned()
}

/// The chat about one pull request or issue, as `OpenThreadChat` gives it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThreadChat {
    pub chat_id: Uuid,
    pub kind: GitItemKind,
    /// `org/repo`.
    pub repo: String,
    pub number: u64,
    pub title: String,
}

/// A pull request or issue as `GetProjectThread` reads it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThreadView {
    pub kind: GitItemKind,
    pub number: u64,
    pub title: String,
    /// Its web page.
    pub url: String,
    /// The login of who opened it.
    pub author: String,
    /// The opening post, cut short when long.
    pub body: String,
    /// Someone other than the caller, bots aside, wrote after the caller.
    pub needs_reply: bool,
    /// The newest comments and reviews, oldest first.
    pub comments: Vec<ThreadPost>,
    /// How many older comments and reviews `comments` leaves out.
    pub earlier: u32,
}

/// A comment or review on a pull request or issue.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThreadPost {
    /// The writer's login.
    pub author: String,
    pub at: SystemTime,
    /// Cut short when long.
    pub body: String,
}

/// A pull request or issue as `GetThreadDetail` reads it. Its text is the
/// host's, written by anyone: show it as data, never as markup.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThreadDetail {
    pub kind: GitItemKind,
    pub number: u64,
    pub title: String,
    /// Its web page.
    pub url: String,
    /// The login of who opened it.
    pub author: String,
    pub state: ThreadState,
    pub draft: bool,
    pub labels: Vec<String>,
    pub opened_at: SystemTime,
    /// The opening post, Markdown, cut short when long.
    pub body: String,
    /// Comments and reviews, oldest first; the newest when there are many.
    pub comments: Vec<ThreadPost>,
    /// How many older comments and reviews `comments` leaves out.
    pub earlier: u32,
    /// A pull request's review decision; `None` for an issue or where the
    /// host does not say.
    pub review: GitReview,
    /// How a pull request's checks stand on its head commit, where the host
    /// says.
    pub checks: Option<RunState>,
}

wire_enum! {
    /// Whether a pull request or issue is open.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::IntoStaticStr)]
    #[strum(serialize_all = "snake_case")]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum ThreadState {
        0 Open {}
        1 Closed {}
        2 Merged {}
    }
}

/// How `TrackProjectIssue` starts a worker on the issue.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IssueStart {
    /// The coding agent: `claude`, `codex`, `gemini`, `pi` or `flsh`.
    pub agent: String,
    /// The `Machine::id` to run on; `None` picks one at start.
    pub node_id: Option<String>,
}

/// One task of a project.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectTask {
    pub id: Uuid,
    pub slug: String,
    pub title: String,
    pub body: String,
    pub state: ProjectTaskState,
    /// The task that must finish first.
    pub depends_on: Option<Uuid>,
    pub issue_number: Option<u64>,
    pub issue_url: Option<String>,
    pub branch: Option<String>,
    pub pr_url: Option<String>,
    pub created_at: SystemTime,
    pub updated_at: SystemTime,
}

wire_enum! {
    /// Where a project task stands.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr)]
    #[strum(serialize_all = "lowercase")]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum ProjectTaskState {
        0 Planned {}
        1 Running {}
        2 Blocked {}
        3 Review {}
        4 Done {}
        5 Dropped {}
    }
}

/// A coding agent working on a project task.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectWorker {
    pub id: Uuid,
    pub task_id: Uuid,
    /// The `Machine::id` it runs on.
    pub machine_id: String,
    pub machine_name: String,
    /// The agent, such as `claude` or `codex`.
    pub agent: String,
    /// The node's id for its session, once it has started.
    pub session: Option<String>,
    pub folder: String,
    pub state: ProjectWorkerState,
    /// The last thing it said, cut short.
    pub last_output: Option<String>,
    pub persona: Option<String>,
    pub started_at: SystemTime,
    pub ended_at: Option<SystemTime>,
}

wire_enum! {
    /// What a project worker is doing.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr)]
    #[strum(serialize_all = "lowercase")]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum ProjectWorkerState {
        0 Starting {}
        1 Running {}
        2 Waiting {}
        3 Ended {}
        4 Failed {}
    }
}

/// A machine that can run coding agents, as `ListAgentHosts` lists it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentHost {
    /// The `Machine::id` it is.
    pub machine_id: String,
    pub machine_name: String,
    /// The agents it offers, such as `claude` and `codex`.
    pub agents: Vec<String>,
    /// The user's environment folder, when this machine is it.
    pub workspace: Option<String>,
    /// Its home folder, as it last reported it.
    pub home: Option<String>,
    pub capabilities: Vec<MachineCapability>,
}

/// The session `StartAgentSession` started.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StartedAgent {
    pub session: MachineSession,
    /// Why it runs on that machine, such as "It has Xcode." Empty when the
    /// caller named the machine.
    pub why: String,
}

/// One session in full, as `GetAgentSession` reads it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentSessionView {
    pub session: MachineSession,
    pub plan: Vec<AgentPlanStep>,
    /// Files it changed, newest last.
    pub files: Vec<String>,
    /// Its last message, cut short.
    pub last_message: String,
    /// Why it failed or its last turn went wrong.
    pub error: Option<String>,
    /// Messages waiting for the running turn to end.
    pub queued: u32,
    pub turns: u32,
    pub entries: Vec<AgentEntry>,
    /// Transcript entries before `entries`.
    pub earlier: u32,
}

/// One step of a session's plan.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentPlanStep {
    pub content: String,
    pub status: AgentPlanStatus,
}

wire_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr)]
    #[strum(serialize_all = "snake_case")]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum AgentPlanStatus {
        0 Pending {}
        1 InProgress {}
        2 Completed {}
    }
}

/// One entry of a session's transcript.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentEntry {
    pub kind: AgentEntryKind,
    pub text: String,
}

wire_enum! {
    /// Who an entry is from: a prompt, the agent's text, a tool call, a
    /// permission question and its answer, or what the agent thought and
    /// for how many milliseconds (`None` while it still thinks, or when
    /// that is not known).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr)]
    #[strum(serialize_all = "lowercase")]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum AgentEntryKind {
        0 User {}
        1 Agent {}
        2 Tool {}
        3 Question {}
        4 Other {}
        5 Thought { ms: Option<u64> } since THOUGHTS
    }
}

wire_enum! {
    /// What `SteerAgentSession` does to a session: `Prompt` sends it a
    /// message after the running turn, `Cancel` stops the turn and drops
    /// waiting messages, `Close` ends the session and stops the agent.
    /// `PromptImages` is `Prompt` with images after the text, as
    /// `check_agent_images` allows them; `text` may then be empty. A
    /// server older than `Feature::AGENT_IMAGES` refuses it, and so does an
    /// agent that takes no images. `Configure` switches the running session
    /// to `model` and thinking `effort` (either may be `None` to keep it),
    /// from its next turn; a server older than `Feature::AGENT_CONFIGURE`
    /// refuses it, and so does a node or agent that cannot switch.
    /// `Steer` is `PromptImages` for the running turn: the node delivers it
    /// once the agent's current tool call finishes rather than after the
    /// whole turn, and like `Prompt` when no turn runs. A server older than
    /// `Feature::AGENT_STEER` refuses it. `DropSteers` takes back the steers
    /// not yet delivered.
    #[derive(Debug, Clone, PartialEq, Eq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum AgentSteer {
        0 Prompt { text: String }
        1 Cancel {}
        2 Close {}
        3 PromptImages { text: String, images: Vec<Upload> } since AGENT_IMAGES
        4 Configure { model: Option<ModelId>, effort: Option<AgentEffort> } since AGENT_CONFIGURE
        5 Steer { text: String, images: Vec<Upload> } since AGENT_STEER
        6 DropSteers {} since AGENT_STEER
    }
}

/// The image types `AgentSteer::PromptImages` may carry.
pub const AGENT_IMAGE_TYPES: &[&str] = &["image/png", "image/jpeg", "image/gif", "image/webp"];
/// The most images one `AgentSteer::PromptImages` may carry.
pub const MAX_AGENT_IMAGES: usize = 5;
/// The largest one image may be.
pub const MAX_AGENT_IMAGE: usize = 5 * 1024 * 1024;
/// The most all of a message's images may hold, so the request stays
/// under a frame.
pub const MAX_AGENT_IMAGES_TOTAL: usize = 7 * 1024 * 1024;

/// Refuses images an agent prompt may not carry: too many, too large, or
/// not a PNG, JPEG, GIF or WebP whose bytes say so too.
pub fn check_agent_images(images: &[Upload]) -> Result<(), ApiError> {
    if images.len() > MAX_AGENT_IMAGES {
        return Err(ApiError::bad_request(format!(
            "At most {MAX_AGENT_IMAGES} images go with one message."
        )));
    }
    let mut total = 0usize;
    for image in images {
        let name = if image.name.is_empty() {
            "An image"
        } else {
            image.name.as_str()
        };
        if !AGENT_IMAGE_TYPES.contains(&image.mime.as_str()) {
            return Err(ApiError::bad_request(format!(
                "{name} is not a PNG, JPEG, GIF or WebP image."
            )));
        }
        if image.data.len() > MAX_AGENT_IMAGE {
            return Err(ApiError::bad_request(format!(
                "{name} is over {} MB; images up to {} MB go.",
                MAX_AGENT_IMAGE >> 20,
                MAX_AGENT_IMAGE >> 20
            )));
        }
        if sniff_image(&image.data) != Some(image.mime.as_str()) {
            return Err(ApiError::bad_request(format!(
                "{name} is not the {} it says it is.",
                image.mime
            )));
        }
        total = total.saturating_add(image.data.len());
    }
    if total > MAX_AGENT_IMAGES_TOTAL {
        return Err(ApiError::bad_request(format!(
            "The images are over {} MB together.",
            MAX_AGENT_IMAGES_TOTAL >> 20
        )));
    }
    Ok(())
}

/// The image type `data` starts like, of `AGENT_IMAGE_TYPES`.
pub fn sniff_image(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if data.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if data.len() >= 12 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

/// An approval waiting in one chat, as `ListPendingApprovals` lists it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatApproval {
    pub session_id: Uuid,
    /// The chat's title; empty when the chat is gone.
    pub session_title: String,
    pub pending: PendingApproval,
}

/// A coding-agent session on one of the user's machines, as
/// `ListAgentSessions` lists it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MachineSession {
    /// The `Machine::id` it runs on.
    pub machine_id: String,
    pub machine_name: String,
    /// The node's id for it, or the agent's own for an external session.
    pub id: String,
    /// The agent, such as `claude` or `codex`.
    pub agent: String,
    pub title: Option<String>,
    pub cwd: String,
    pub status: AgentStatus,
    /// What the agent is doing now, such as "Edit src/main.rs".
    pub doing: Option<String>,
    pub started_at: SystemTime,
    pub last_activity: SystemTime,
    /// Started outside Beep, such as Claude Code in a terminal.
    pub external: bool,
    /// What it waits on an answer to; never set for an external session.
    pub question: Option<AgentQuestion>,
}

wire_enum! {
    /// What a session is doing. `Waiting` and `Running` are only for
    /// external sessions: waiting for the user, or running with nothing
    /// more known.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr)]
    #[strum(serialize_all = "lowercase")]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum AgentStatus {
        0 Starting {}
        1 Working {}
        2 Asking {}
        3 Idle {}
        4 Ended {}
        5 Waiting {}
        6 Running {}
    }
}

/// A permission question a session asks before it acts.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentQuestion {
    pub id: u32,
    /// What the agent wants to do, such as "Run cargo test".
    pub text: String,
    /// The command, path or other input it would act on, cut short.
    pub detail: Option<String>,
    pub options: Vec<AgentOption>,
}

/// One answer to an `AgentQuestion`.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentOption {
    pub id: String,
    pub name: String,
    /// It lets the agent go ahead, once or always.
    pub allow: bool,
}

/// The apps, graphics cards and abilities of one machine, as
/// `ListMachineDetails` lists them.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MachineDetails {
    /// The `Machine::id` it describes.
    pub id: String,
    pub apps: Vec<MachineApp>,
    pub gpus: Vec<MachineGpu>,
    pub capabilities: Vec<MachineCapability>,
}

/// An app installed on a machine.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MachineApp {
    /// Its display name, such as "Blender" or "Unreal Engine 5.4".
    pub name: String,
    pub version: Option<String>,
}

/// A graphics card in a machine.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MachineGpu {
    /// Its maker, such as "NVIDIA" or "Apple".
    pub vendor: String,
    pub model: String,
    /// Video memory in MB, when known.
    pub vram_mb: Option<u64>,
    pub driver: Option<String>,
}

/// A machine's live resources, as `ListMachineStats` lists them.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MachineStats {
    /// The `Machine::id` it describes.
    pub id: String,
    /// When the server got the sample.
    pub sampled_at: SystemTime,
    /// The workspace's volume first, when the machine keeps one, then the
    /// home folder's when it is on another volume.
    pub disks: Vec<MachineDisk>,
    pub memory_used_mb: u64,
    pub memory_total_mb: u64,
    /// The load average over the last minute, when the machine gives one.
    pub load: Option<f64>,
}

/// How full the volume holding one folder is.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MachineDisk {
    /// The folder it was measured at.
    pub path: String,
    /// What a user may still write, in bytes.
    pub free_bytes: u64,
    pub total_bytes: u64,
}

wire_enum! {
    /// What a machine can be asked to do.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr)]
    #[strum(serialize_all = "lowercase")]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum MachineCapability {
        0 Gpu {}
        1 Cuda {}
        2 Blender {}
        3 Unreal {}
        4 Xcode {}
        5 Android {}
        6 Docker {}
    }
}

impl MachineCapability {
    /// Its name for people, such as "Xcode" or "GPU".
    pub fn label(self) -> &'static str {
        match self {
            Self::Gpu {} => "a GPU",
            Self::Cuda {} => "CUDA",
            Self::Blender {} => "Blender",
            Self::Unreal {} => "Unreal Engine",
            Self::Xcode {} => "Xcode",
            Self::Android {} => "Android tools",
            Self::Docker {} => "Docker",
        }
    }
}

/// A problem the improvement watcher found in users' chats, as
/// `ListFindings` lists it. The same problem in other chats counts here
/// once more rather than coming again.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    pub id: String,
    /// What went wrong: `correction`, `failed_tool`, `wrong_tool`,
    /// `could_have_found`, `loop`, `slow`, `gave_up` or `other`. Show one
    /// this app does not know as other.
    pub kind: String,
    pub summary: String,
    /// How to make it happen again.
    pub steps: String,
    /// Where it likely comes from: `prompt`, `tool_schema`, `missing_tool`,
    /// `model` or `other`.
    pub cause: String,
    /// Why the watcher thinks so.
    pub cause_detail: String,
    /// What it proposes to change.
    pub fix: String,
    pub status: FindingStatus,
    /// How many times it was seen.
    pub seen: u32,
    pub first_seen: SystemTime,
    pub last_seen: SystemTime,
    /// The caller's chat it was last seen in, `None` when it was only seen
    /// in other users' chats.
    pub session_id: Option<Uuid>,
    /// The ids of the first and last messages of that chat that show it
    /// (`ChatMessage::id`); both 0 without a chat.
    pub first_message: i64,
    pub last_message: i64,
}

wire_enum! {
    /// Where the user left a finding.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum FindingStatus {
        0 Open {}
        1 Resolved {}
        2 Dismissed {}
    }
}

/// What a tool step showed while it ran, as `GetToolLabels` lists it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolLabel {
    pub call_id: String,
    /// The `RunEvent::ToolStarted` `description` of the call.
    pub description: String,
}

/// A sub-agent or background job of a chat, as `ListWork` lists it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkItem {
    /// As the agent writes it, such as "w12". `CancelWork` takes it.
    pub id: String,
    pub kind: WorkKind,
    /// What it was asked to do, cut short.
    pub task: String,
    pub started_at: SystemTime,
    /// What it did last, such as the tool it called. Empty before its
    /// first step.
    pub step: String,
    /// How it ended; `None` while it runs.
    pub outcome: Option<WorkOutcome>,
}

/// How a sub-agent or job ended.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkOutcome {
    pub ok: bool,
    /// Its report or result, or why it failed.
    pub text: String,
}

wire_enum! {
    /// What runs in the background: a sub-agent the agent started, or one
    /// slow tool call it moved there.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum WorkKind {
        0 Agent {}
        1 Job {}
    }
}

/// The longest text `Say` speaks.
pub const MAX_SAY_CHARS: usize = 600;

impl Duplex for Transcribe {
    type Up = serde_bytes::ByteBuf;
}

/// The only rate `Transcribe` takes.
pub const TRANSCRIBE_RATE: u32 = 16_000;
/// The most audio one `Transcribe` may carry: five minutes at `TRANSCRIBE_RATE`.
pub const MAX_TRANSCRIBE_BYTES: usize = 5 * 60 * 16_000 * 2;

wire_enum! {
    /// What `Transcribe` heard: each `Delta` adds `text` to the words so
    /// far, and `Done` gives the whole text once the audio ended.
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Transcription {
        0 Delta { text: String }
        1 Done { text: String }
        2 Unavailable {}
    }
}

/// The most a `Chat`'s `voice` may hold: a minute of 16 kHz mono WAV, with
/// room to spare.
pub const MAX_VOICE: usize = 3 * 1024 * 1024;

/// The audio types a `Chat`'s `voice` may be.
pub const VOICE_TYPES: &[&str] = &[
    "audio/wav",
    "audio/x-wav",
    "audio/mpeg",
    "audio/mp4",
    "audio/m4a",
    "audio/aac",
    "audio/ogg",
    "audio/webm",
    "audio/flac",
];

/// A notice read out: the words spoken and the audio, an MP3.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VoiceNote {
    pub text: String,
    pub audio: Upload,
}

/// The most a message's `files` may hold in all, so the request stays
/// under a frame.
pub const MAX_UPLOAD: usize = 5 * 1024 * 1024;

/// A file sent with a message, such as a PDF or a spreadsheet.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Upload {
    pub name: String,
    pub mime: String,
    #[serde(with = "serde_bytes")]
    pub data: Vec<u8>,
}

/// A file someone sent with a message, kept on the server as long as its
/// chat. Read its bytes with `GetFile`.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileRef {
    pub id: String,
    pub name: String,
    pub mime: String,
    pub size: u64,
}

/// The files one message carried, in the order they were sent.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SentFiles {
    /// The `HistoryEntry::User` `timestamp`.
    pub message: i64,
    pub files: Vec<FileRef>,
}

/// One connected account of an app such as Fastmail.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppAccount {
    pub id: String,
    /// The account's email address, once the server has read it.
    pub address: Option<String>,
}

/// A remote MCP server the user added. The server makes every call, so it
/// works the same from every device.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpServer {
    pub id: String,
    pub name: String,
    pub url: String,
    pub connected: bool,
    /// Whether it signs in with OAuth.
    pub sign_in: bool,
    /// The tools Beep gets from it, as the server names them.
    pub tools: Vec<String>,
}

/// A research: a lead agent plans it in its chat, scouts research the
/// plan's questions, a critic challenges them and a writer drafts a paper.
/// The chat holds only the plan and the result; the agents' work is here.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Research {
    pub id: Uuid,
    pub topic: String,
    pub status: ResearchStatus,
    /// The team, lead first. Only the lead until the plan is approved.
    pub agents: Vec<ResearchAgent>,
    /// Log entries after the request's `after`, oldest first.
    pub entries: Vec<ResearchEntry>,
    /// Every source the scouts kept, numbered as the notes and draft cite them.
    pub sources: Vec<ResearchSource>,
    /// The writer's paper in Markdown, empty until it starts.
    pub draft: String,
    /// In milliseconds since the Unix epoch.
    pub started_at: i64,
    /// When the agents stopped, for any reason.
    pub finished_at: Option<i64>,
    /// The newest round, from 1: each follow-up the team researches is one.
    pub round: u32,
}

wire_enum! {
    /// `Planning` while the lead and the user agree on a plan; `Interrupted`
    /// when the server restarted while the agents ran.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum ResearchStatus {
        0 Planning {}
        1 Researching {}
        2 Reviewing {}
        3 Writing {}
        4 Done {}
        5 Failed {}
        6 Stopped {}
        7 Interrupted {}
    }
}

/// A research as a list shows it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResearchSummary {
    /// The chat the lead talks to the user in.
    pub session_id: Uuid,
    pub topic: String,
    pub status: ResearchStatus,
    /// The newest round, from 1.
    pub round: u32,
    /// How many sources the scouts kept.
    pub sources: u32,
    /// In milliseconds since the Unix epoch.
    pub started_at: i64,
    pub finished_at: Option<i64>,
}

/// One agent of a research.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResearchAgent {
    /// `lead`, `scout-1`, ..., `critic` or `writer`.
    pub id: String,
    /// Like "Scout 1".
    pub name: String,
    /// What it does, like the question a scout works on.
    pub role: String,
    /// The display name of the model it runs on.
    pub model: String,
    /// The round a scout researched in; 0 for agents that work in every
    /// round.
    pub round: u32,
}

/// One thing an agent did or said.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResearchEntry {
    /// Grows with each entry of the research.
    pub seq: i64,
    /// The agent's id.
    pub agent: String,
    pub body: ResearchEntryBody,
    /// In milliseconds since the Unix epoch.
    pub timestamp: i64,
}

wire_enum! {
    /// `Said` is a message to the room, to one agent or to all when `to` is
    /// `None`. `Step` is a tool the agent ran, in its own thread. `Note` is a
    /// finding for the notes, citing source numbers.
    #[derive(Debug, Clone, PartialEq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum ResearchEntryBody {
        0 Said { to: Option<String>, content: String }
        1 Step { label: String, success: bool }
        2 Note { content: String, sources: Vec<u32> }
    }
}

/// A source a scout kept.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResearchSource {
    /// From 1, as cited.
    pub number: u32,
    pub url: String,
    pub title: String,
    /// The passage it was kept for.
    pub quote: String,
    /// The agent that found it.
    pub agent: String,
}

/// A web page a reply used: one Beep read, or a search result.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebSource {
    /// An http(s) URL.
    pub url: String,
    /// The page's title, empty when unknown.
    pub title: String,
}

/// The web pages one turn used, pages read first, at most `MAX_SOURCES`.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TurnSources {
    /// The `HistoryEntry::User` `timestamp` of the turn.
    pub message: i64,
    pub sources: Vec<WebSource>,
}

/// The most sources a reply shows.
pub const MAX_SOURCES: usize = 6;

/// The tools of an added server, in the order it lists them.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpTools {
    pub tools: Vec<McpTool>,
    /// How many tools of one server the agent works well with. Until the
    /// user picks, the first `limit` usable tools are on.
    pub limit: u32,
}

#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpTool {
    /// The tool's name on its server.
    pub name: String,
    pub description: String,
    /// Whether the agent gets it.
    pub enabled: bool,
    /// False when Beep cannot use it, such as a schema too large to send
    /// the model. It stays off.
    pub usable: bool,
}

/// An added or reconnected server: connected, or waiting for the user to
/// sign in at `auth_url`.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpConnect {
    pub server: McpServer,
    pub auth_url: Option<String>,
}

/// What a `get_route` call found, as the route card draws it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteCard {
    /// One to three ways there, best first, as the model was told them.
    pub routes: Vec<RouteOption>,
}

#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteOption {
    /// Clock times where they happen, like "19:01".
    pub departs: String,
    pub arrives: String,
    pub minutes: u32,
    pub transfers: u32,
    /// Whether a leg has live times rather than the timetable's.
    pub live: bool,
    pub legs: Vec<RouteLeg>,
}

/// One walk or ride of a route.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteLeg {
    /// The router's mode in lowercase: "walk", "bus", "tram", "subway",
    /// "rail", "ferry" and others. Text rather than an enum so a mode the
    /// router adds later reaches older apps as an unknown ride.
    pub mode: String,
    /// The line's short name, like "G". Empty for a walk.
    pub line: String,
    /// The line's long name, like "Ginza Line". Empty when the feed has none.
    pub name: String,
    /// "#RRGGBB", empty when the feed gives none.
    pub color: String,
    /// "#RRGGBB" for text on `color`, empty when the feed gives none.
    pub text_color: String,
    pub headsign: String,
    /// Stop names. "" stands for the start or the destination themselves.
    pub from: String,
    pub to: String,
    /// The platform or track, empty when unknown.
    pub from_track: String,
    pub to_track: String,
    pub departs: String,
    pub arrives: String,
    pub minutes: u32,
    /// Stops ridden, counting the last one. Zero for a walk.
    pub stops: u32,
    /// Zero when unknown.
    pub meters: u32,
    /// The leg's shape, simplified to what a small sketch needs.
    pub points: Vec<RoutePoint>,
    /// The station codes signs show, like "G10" in Tokyo. Empty when the
    /// feed has none. Cards saved before codes lack these and `between`.
    #[serde(default)]
    pub from_code: String,
    #[serde(default)]
    pub to_code: String,
    /// The stops ridden through, in order, not counting `from` and `to`.
    #[serde(default)]
    pub between: Vec<RouteStop>,
}

/// A stop a ride passes.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteStop {
    pub name: String,
    /// The station code, empty when the feed has none.
    pub code: String,
    /// The clock time there, like "19:08".
    pub time: String,
    pub at: RoutePoint,
}

#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RoutePoint {
    pub latitude: f64,
    pub longitude: f64,
}

/// `RouteCard` as wire 2 apps read it from `GetRoute`. Frozen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteCardV1 {
    pub routes: Vec<RouteOptionV1>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteOptionV1 {
    pub departs: String,
    pub arrives: String,
    pub minutes: u32,
    pub transfers: u32,
    pub live: bool,
    pub legs: Vec<RouteLegV1>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteLegV1 {
    pub mode: String,
    pub line: String,
    pub name: String,
    pub color: String,
    pub text_color: String,
    pub headsign: String,
    pub from: String,
    pub to: String,
    pub from_track: String,
    pub to_track: String,
    pub departs: String,
    pub arrives: String,
    pub minutes: u32,
    pub stops: u32,
    pub meters: u32,
    pub points: Vec<RoutePoint>,
}

impl From<RouteCard> for RouteCardV1 {
    fn from(card: RouteCard) -> Self {
        Self {
            routes: card
                .routes
                .into_iter()
                .map(|route| RouteOptionV1 {
                    departs: route.departs,
                    arrives: route.arrives,
                    minutes: route.minutes,
                    transfers: route.transfers,
                    live: route.live,
                    legs: route
                        .legs
                        .into_iter()
                        .map(|leg| RouteLegV1 {
                            mode: leg.mode,
                            line: leg.line,
                            name: leg.name,
                            color: leg.color,
                            text_color: leg.text_color,
                            headsign: leg.headsign,
                            from: leg.from,
                            to: leg.to,
                            from_track: leg.from_track,
                            to_track: leg.to_track,
                            departs: leg.departs,
                            arrives: leg.arrives,
                            minutes: leg.minutes,
                            stops: leg.stops,
                            meters: leg.meters,
                            points: leg.points,
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}

/// A chart under a reply, as `show_chart` and `show_stock_chart` return it
/// in their tool result and `StockChart` answers it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chart {
    pub title: String,
    pub kind: ChartKind,
    /// Whether `x` is a time in Unix milliseconds or an index into
    /// `categories`.
    pub x: ChartAxis,
    /// The labels of a category axis, in order. Empty on a time axis.
    pub categories: Vec<String>,
    /// What the values measure, such as "USD" or "°C". Empty when unknown.
    pub unit: String,
    /// One to four series, each drawn in its own color.
    pub series: Vec<ChartSeries>,
    /// Set on a stock chart, so the app can offer the other ranges.
    pub stock: Option<StockInfo>,
    /// Where the numbers come from, such as "Yahoo Finance". Empty when
    /// the model gave them.
    pub source: String,
}

#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChartSeries {
    pub name: String,
    pub points: Vec<ChartPoint>,
}

#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ChartPoint {
    pub x: f64,
    pub y: f64,
}

/// The quote a stock chart is drawn for.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StockInfo {
    pub symbol: String,
    /// The company or fund, such as "Apple Inc.". Empty when unknown.
    pub name: String,
    pub exchange: String,
    pub currency: String,
    pub range: ChartRange,
    /// The latest price.
    pub last: f64,
    /// The close before the range started, which the change is against.
    pub previous: f64,
}

#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChartKind {
    Line,
    Area,
    Bar,
}

#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChartAxis {
    Time,
    Category,
}

/// How far back a stock chart reaches.
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChartRange {
    #[serde(rename = "1d")]
    Day,
    #[serde(rename = "5d")]
    Week,
    #[serde(rename = "1mo")]
    Month,
    #[serde(rename = "6mo")]
    HalfYear,
    #[serde(rename = "ytd")]
    YearToDate,
    #[serde(rename = "1y")]
    Year,
    #[serde(rename = "5y")]
    FiveYears,
    Max,
}

impl ChartRange {
    /// The name it has in JSON and in Yahoo Finance's `range`, such as "1mo".
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Day => "1d",
            Self::Week => "5d",
            Self::Month => "1mo",
            Self::HalfYear => "6mo",
            Self::YearToDate => "ytd",
            Self::Year => "1y",
            Self::FiveYears => "5y",
            Self::Max => "max",
        }
    }
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

/// The agent one turn ran as.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TurnAgent {
    /// The `HistoryEntry::User` `timestamp` of the turn.
    pub message: i64,
    /// The agent's name, like "Shopping". `None` for General and for a turn
    /// that ran without agents.
    pub agent: Option<String>,
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

/// Where MCP sign-ins come back. The Worker hands it to the app: Android
/// claims it as an App Link, and the page sends other apps on to
/// `beep://oauth?...`.
pub fn mcp_callback_url() -> String {
    format!("https://{LINK_HOST}/oauth/callback")
}

/// What a sign-in redirect brought back.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpCallback {
    pub code: String,
    pub state: String,
    pub issuer: Option<String>,
}

/// The sign-in result in `url`, if it is `mcp_callback_url()` or
/// `beep://oauth` with a `code` and a `state`.
pub fn mcp_callback(url: &str) -> Option<McpCallback> {
    let url = url.trim();
    let callback = format!("https://{LINK_HOST}/oauth/callback");
    let query = url
        .strip_prefix(&callback)
        .or_else(|| url.strip_prefix("beep://oauth"))?;
    let query = query.strip_prefix('/').unwrap_or(query).strip_prefix('?')?;
    let (mut code, mut state, mut issuer) = (None, None, None);
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let value = percent_decode(value)?;
        match key {
            "code" => code = Some(value),
            "state" => state = Some(value),
            "iss" => issuer = Some(value),
            _ => {}
        }
    }
    Some(McpCallback {
        code: code.filter(|code| !code.is_empty())?,
        state: state.filter(|state| !state.is_empty())?,
        issuer,
    })
}

/// A query value with `%XX` escapes and `+` decoded, or `None` when it is
/// not valid UTF-8 after.
fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' => {
                let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 2;
            }
            byte => out.push(byte),
        }
        i += 1;
    }
    String::from_utf8(out).ok()
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
        11 Transcript { text: String } since VOICE
        12 Sources { sources: Vec<WebSource> } since SOURCES
    }
}

wire_enum! {
    /// Work the server asks the phone to do during a run. `Approve` asks the user to
    /// allow a tool call, answered as `{"decision": "allow"}`, where the
    /// decision is `allow`, `always` or `deny`. `Browser` and `MacFiles` are
    /// unused: the agent no longer drives the in-app browser or reads Mac
    /// files, so the server never sends them. They stay so released apps
    /// still decode the enum.
    #[derive(Debug, Clone, PartialEq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum DeviceQuery {
        0 Contacts { name: String }
        1 Browser { command: BrowserCommand }
        3 Approve { approval: Approval }
        4 MacFiles { command: MacFileCommand } since MAC_FILES
    }
}

wire_enum! {
    /// What Beep asks the Mac app to do with the user's files. Paths are
    /// absolute or start at `~`, and stay under the home folder. `List`
    /// answers a folder's entries as a JSON list of `name`, `folder`, `size`
    /// and `modified`; `Find` the paths under `folder` whose name contains
    /// `name`, as a JSON list; `Read` a file as JSON with `name`, `mime`,
    /// `size` and base64 `data`, up to `MAC_READ` bytes. Unused: kept for
    /// wire compatibility with released apps, never sent.
    #[derive(Debug, Clone, PartialEq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum MacFileCommand {
        0 List { path: String }
        1 Find { name: String, folder: String }
        2 Read { path: String }
    }
}

/// The largest file `MacFileCommand::Read` answers with, so its base64
/// answer stays under a frame.
pub const MAC_READ: u64 = 5 * 1024 * 1024;

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

impl ResearchStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Planning {} => "planning",
            Self::Researching {} => "researching",
            Self::Reviewing {} => "reviewing",
            Self::Writing {} => "writing",
            Self::Done {} => "done",
            Self::Failed {} => "failed",
            Self::Stopped {} => "stopped",
            Self::Interrupted {} => "interrupted",
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

/// The models the caller can pick from and the one their turns use.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Models {
    /// Empty on servers from before every user could pick.
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

/// Whether agents are on for everyone, and the agents Jev picks from.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Agents {
    pub enabled: bool,
    /// General first. Empty when the server has no agents set up, and then
    /// they cannot be turned on.
    pub agents: Vec<AgentInfo>,
}

/// An agent: a system prompt, a model and a thinking level for one kind of task.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentInfo {
    /// Such as `getting_around`.
    pub id: String,
    pub name: String,
    /// What it handles.
    pub when: String,
    /// A `ModelChoice::id`.
    pub model: String,
    /// The least it thinks.
    pub thinking: ReasoningLevel,
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
    /// An action for the in-app browser on the phone. Unused: kept for wire
    /// compatibility with released apps, never sent.
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
    /// What kind of client a key belongs to.
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

wire_enum! {
    /// The APNs server an iPhone's push token belongs to. Builds run from
    /// Xcode get sandbox tokens; TestFlight and App Store builds production ones.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum ApnsEnvironment {
        0 Sandbox {}
        1 Production {}
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

/// A machine running beep-node, and what it last reported.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Machine {
    /// Its key, the `LinkedDevice::id` it signs in with.
    pub id: String,
    pub name: String,
    /// Connected to the server now.
    pub online: bool,
    /// When it last connected or reported.
    pub seen_at: SystemTime,
    /// The beep-node version, such as "0.1.0".
    pub version: String,
    /// `None` until its first report.
    pub env: Option<MachineEnv>,
}

/// What a machine reported about itself.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MachineEnv {
    /// `linux` or `macos`.
    pub os: String,
    /// Such as "Ubuntu 24.04" or "macOS 26.3".
    pub os_version: String,
    pub arch: String,
    pub hostname: String,
    pub cpus: u32,
    pub memory_mb: u64,
    /// The folder commands start in.
    pub home: String,
    pub tools: Vec<MachineTool>,
    pub logins: Vec<MachineLogin>,
    /// Android virtual devices.
    pub avds: Vec<String>,
}

/// A known tool found on a machine's `PATH`.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MachineTool {
    pub name: String,
    /// The first line of its version output.
    pub version: Option<String>,
}

/// Whether a CLI on a machine is signed in, such as `gh`.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MachineLogin {
    pub tool: String,
    pub signed_in: bool,
    pub account: Option<String>,
}

/// A standing permission the user approved once for one routine: each run
/// of the routine may make up to `per_run` calls of one action without
/// asking, when the call matches what the user approved. Anything else
/// still asks.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Grant {
    pub id: String,
    /// The `Cron::id` of the routine it belongs to.
    pub routine_id: String,
    /// The routine's label, or its prompt when it has none.
    pub routine: String,
    /// The app's name, such as "Gmail".
    pub app: String,
    /// What it allows, such as "reply to thread".
    pub action: String,
    /// What the user approved, labelled for reading, as the card showed it.
    pub fields: Vec<ApprovalField>,
    pub per_run: u32,
    pub expires_at: SystemTime,
    pub created_at: SystemTime,
    /// When a call last used it; `None` before the first.
    pub last_used_at: Option<SystemTime>,
}

/// A memory that is about something other than the user.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScopedMemory {
    /// The `Memory::id`.
    pub id: String,
    pub scope: MemoryScope,
}

wire_enum! {
    /// What a memory is about when it is not the user: a machine by its key,
    /// with its name while the machine is known; a project, such as a
    /// repository, by the name Beep knows it by; or one of the user's
    /// accounts by app, such as `github`.
    #[derive(Debug, Clone, PartialEq, Eq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum MemoryScope {
        0 Machine { id: String, name: Option<String> }
        1 Project { name: String }
        2 Account { app: String }
    }
}

/// A code that lets another device join the account.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceInvite {
    /// Ten letters and digits, as `link::format_join_code` shows them.
    pub code: String,
    /// The code as text for a QR code, as `link::join_uri` makes it.
    pub uri: String,
    pub expires_at: SystemTime,
}

/// The device behind an invite, shown before joining.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JoinPreview {
    pub name: String,
    pub kind: DeviceKind,
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
    /// The chat or group the notice is about. On a computer it may instead
    /// be an `AgentNoticeKey`, for a coding-agent notice.
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

impl Notice {
    /// A background question that stopped waiting, answered on some device
    /// or withdrawn: `session_id` is its chat, and `approval` names it, to
    /// clear its card. Such a notice also has `changed` set and no title,
    /// so apps from before it only refresh the chat.
    pub fn resolved(session_id: String, question: PendingApproval) -> Self {
        Self {
            title: String::new(),
            body: String::new(),
            session_id,
            app: None,
            approval: Some(question),
            changed: true,
            group: false,
        }
    }

    /// The `query_id` of the question this notice says stopped waiting, or
    /// `None` for any other notice.
    pub fn resolved_question(&self) -> Option<&str> {
        match &self.approval {
            Some(question) if self.changed && !self.group => Some(&question.query_id),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_resolved_notice_names_a_question() {
        let question = PendingApproval {
            run_id: String::new(),
            query_id: "q1".into(),
            approval: Approval {
                app: "Gmail".into(),
                action: "Send email".into(),
                fields: Vec::new(),
            },
        };
        let resolved = Notice::resolved("chat".into(), question.clone());
        assert_eq!(resolved.resolved_question(), Some("q1"));
        assert!(resolved.title.is_empty());
        let asking = Notice {
            title: "Allow Gmail: Send email?".into(),
            changed: false,
            ..resolved.clone()
        };
        assert_eq!(asking.resolved_question(), None);
        let group = Notice {
            group: true,
            ..resolved
        };
        assert_eq!(group.resolved_question(), None);
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
    fn share_token_reads_only_share_links() {
        assert_eq!(share_url("ab12"), "https://beep.flashmind.com/s/ab12");
        assert_eq!(
            mcp_callback(
                "https://beep.flashmind.com/oauth/callback?code=a%2Fb&state=s1&iss=https%3A%2F%2Fid.example"
            ),
            Some(McpCallback {
                code: "a/b".to_owned(),
                state: "s1".to_owned(),
                issuer: Some("https://id.example".to_owned()),
            })
        );
        assert_eq!(
            mcp_callback("beep://oauth?state=s1&code=c"),
            Some(McpCallback {
                code: "c".to_owned(),
                state: "s1".to_owned(),
                issuer: None,
            })
        );
        assert_eq!(
            mcp_callback("https://beep.flashmind.com/oauth/callback?code=c"),
            None
        );
        assert_eq!(
            mcp_callback("https://beep.flashmind.com.evil/oauth/callback?code=c&state=s"),
            None
        );
        assert_eq!(mcp_callback("beep://oauth?code=%zz&state=s"), None);
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
    fn a_request_skips_fields_a_newer_app_added() {
        // GetRun with a second field this build does not know.
        let buf = [0x92, 0xa2, b'r', b'1', 0x91, 0x07];
        assert_eq!(
            rmp_serde::from_slice::<GetRun>(&buf).unwrap(),
            GetRun { id: "r1".into() }
        );
        // A request with no fields yet, given one.
        assert!(rmp_serde::from_slice::<Health>(&[0x91, 0xc0]).is_ok());
    }

    fn image(name: &str, mime: &str, data: Vec<u8>) -> Upload {
        Upload {
            name: name.into(),
            mime: mime.into(),
            data,
        }
    }

    fn png(size: usize) -> Vec<u8> {
        let mut data = b"\x89PNG\r\n\x1a\n".to_vec();
        data.resize(size.max(data.len()), 0);
        data
    }

    #[test]
    fn agent_images_of_the_four_types_pass() {
        let mut webp = b"RIFF\0\0\0\0WEBPVP8 ".to_vec();
        webp.push(0);
        let images = [
            image("a.png", "image/png", png(100)),
            image("b.jpg", "image/jpeg", vec![0xff, 0xd8, 0xff, 0xe0]),
            image("c.gif", "image/gif", b"GIF89a..".to_vec()),
            image("d.webp", "image/webp", webp),
        ];
        assert_eq!(check_agent_images(&images), Ok(()));
        assert_eq!(check_agent_images(&[]), Ok(()));
    }

    #[test]
    fn agent_images_over_the_limits_are_refused() {
        let six: Vec<Upload> = (0..6)
            .map(|_| image("a.png", "image/png", png(10)))
            .collect();
        let error = check_agent_images(&six).unwrap_err().to_string();
        assert!(error.contains("At most 5 images"), "{error}");

        let big = [image("big.png", "image/png", png(MAX_AGENT_IMAGE + 1))];
        let error = check_agent_images(&big).unwrap_err().to_string();
        assert!(error.contains("big.png is over 5 MB"), "{error}");
        let at_limit = [image("ok.png", "image/png", png(MAX_AGENT_IMAGE))];
        assert_eq!(check_agent_images(&at_limit), Ok(()));

        let two = [
            image("a.png", "image/png", png(4 << 20)),
            image("b.png", "image/png", png(4 << 20)),
        ];
        let error = check_agent_images(&two).unwrap_err().to_string();
        assert!(error.contains("over 7 MB together"), "{error}");
    }

    #[test]
    fn agent_images_of_other_types_are_refused() {
        let svg = [image("a.svg", "image/svg+xml", b"<svg/>".to_vec())];
        let error = check_agent_images(&svg).unwrap_err().to_string();
        assert!(error.contains("a.svg is not a PNG"), "{error}");
        let lying = [image("a.png", "image/png", vec![0xff, 0xd8, 0xff])];
        let error = check_agent_images(&lying).unwrap_err().to_string();
        assert!(error.contains("not the image/png it says"), "{error}");
    }
}
