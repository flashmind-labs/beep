//! Messages exchanged with clients. Each request is a struct declared in
//! `requests!` with its op and reply type. Enums made with `wire_enum!` encode
//! as `[op, field...]`; structs encode as their fields in declaration order.

use chrono::{DateTime, Utc};
use flashmind::types::TokenUsage;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::fmt;
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
    /// First request on every connection; the server closes it on a bad token.
    0 Auth { token: String } => Call<()>
    1 Health {} => Call<()>
    2 Chat { session_id: Uuid, request_id: Uuid, message: String, image: Option<String> } => Call<Run>
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
    12 ListMemories {} => Call<Vec<Memory>>
    13 DeleteMemory { key: String } => Call<()>
    14 ListCrons {} => Call<Vec<Cron>>
    /// Returns the new cron's id.
    15 CreateCron { label: String, prompt: String, schedule: String } => Call<String>
    /// Returns whether the cron is now enabled.
    16 ToggleCron { id: String } => Call<bool>
    17 DeleteCron { id: String } => Call<()>
    18 UpdateCron { id: String, enabled: Option<bool>, label: Option<String>, schedule: Option<String> } => Call<()>
    19 RegisterDevice { device_id: String, fcm_token: String } => Call<()>
    20 UnregisterDevice { device_id: String } => Call<()>
    21 UpdateLocation { latitude: f64, longitude: f64, accuracy: f64, timezone: String } => Call<()>
    /// Returns how many notifications were stored.
    22 SaveNotifications { notifications: Vec<PhoneNotification> } => Call<u64>
    23 StreamNotifications {} => Stream<Notice>
    24 ListIntegrations {} => Call<Vec<Integration>>
    /// Returns the new integration's id.
    25 CreateIntegration { provider: String, label: String, config: String } => Call<String>
    26 DeleteIntegration { id: String } => Call<()>
    27 UpdateCredentials { id: String, credentials: String, status: String } => Call<()>
}

wire_enum! {
    /// Why the server refused or failed a request.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum ApiError {
        0 BadRequest { message: String }
        1 NotFound {}
        2 Conflict { message: String }
        3 Internal {}
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
    /// One entry of a run's event stream. Also the stored form of an event.
    #[derive(Debug, Clone)]
    pub enum RunEvent {
        0 TextDelta { delta: String }
        1 ToolStarted { name: String, call_id: String, description: String }
        2 ToolResult { name: String, call_id: String, output: String, success: bool }
        3 Usage { usage: TokenUsage }
        4 Error { message: String }
        5 DeviceQuery { query_id: String, query: DeviceQuery }
        6 RunFinished { status: String }
        7 RunStarted { session_id: String }
    }
}

wire_enum! {
    /// Work the server asks the phone to do during a run.
    #[derive(Debug, Clone, PartialEq)]
    pub enum DeviceQuery {
        0 Contacts { name: String }
        1 Browser { command: BrowserCommand }
    }
}

wire_enum! {
    /// An action for the in-app browser on the phone.
    #[derive(Debug, Clone, PartialEq)]
    pub enum BrowserCommand {
        0 Navigate { url: String }
        1 Click { selector: String }
        2 Type { selector: String, text: String }
        3 Scroll { direction: ScrollDirection }
        4 GetText { selector: String }
        5 GetPageInfo {}
    }
}

wire_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ScrollDirection {
        0 Up {}
        1 Down {}
    }
}

wire_enum! {
    /// The phone's answer to a `DeviceQuery`.
    #[derive(Debug, Clone, PartialEq)]
    pub enum DeviceAnswer {
        0 Ok { output: String }
        1 Err { message: String }
    }
}

wire_enum! {
    #[derive(Debug, Clone, PartialEq)]
    pub enum HistoryEntry {
        0 User { content: String, timestamp: DateTime<Utc> }
        1 Assistant { content: String, timestamp: DateTime<Utc> }
    }
}

/// One stored entry of a run's event stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub seq: i64,
    pub event: RunEvent,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Run {
    pub id: String,
    pub session_id: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub pinned: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Memory {
    pub key: String,
    pub content: String,
    pub category: String,
}

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
    pub last_status: Option<String>,
    pub last_error: Option<String>,
    pub next_run_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Integration {
    pub id: String,
    pub provider: String,
    pub label: String,
    pub status: String,
    pub created_at: String,
}

/// A notification captured on the phone and uploaded for the assistant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhoneNotification {
    pub app: String,
    pub title: String,
    pub text: String,
    /// Milliseconds since the Unix epoch.
    pub timestamp: i64,
}

/// A message the server pushes to the user's open notification streams.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notice {
    pub title: String,
    pub body: String,
    pub session_id: String,
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
            image: None,
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
