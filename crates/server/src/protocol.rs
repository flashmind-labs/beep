//! Messages exchanged with clients. Enums made with `wire_enum!` encode as
//! `[op, field...]`; structs encode as their fields in declaration order.

use chrono::{DateTime, Utc};
use flashmind::types::TokenUsage;
use serde::{Deserialize, Serialize};
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

wire_enum! {
    #[derive(Debug, Clone, PartialEq)]
    pub enum Request {
        0 Auth { token: String }
        1 Health {}
        2 Chat { session_id: Uuid, request_id: Uuid, message: String, image: Option<String> }
        3 GetRun { id: String }
        4 CancelRun { id: String }
        5 GetEvents { run_id: String, after: i64 }
        6 DeviceResult { run_id: String, query_id: String, answer: DeviceAnswer }
        7 ListSessions {}
        8 GetSession { id: String }
        9 RenameSession { id: String, title: String }
        10 DeleteSession { id: String }
        11 PinSession { id: String, pinned: bool }
        12 ListMemories {}
        13 DeleteMemory { key: String }
        14 ListCrons {}
        15 CreateCron { label: String, prompt: String, schedule: String }
        16 ToggleCron { id: String }
        17 DeleteCron { id: String }
        18 UpdateCron { id: String, enabled: Option<bool>, label: Option<String>, schedule: Option<String> }
        19 RegisterDevice { device_id: String, fcm_token: String }
        20 UnregisterDevice { device_id: String }
        21 UpdateLocation { latitude: f64, longitude: f64, accuracy: f64, timezone: String }
        22 SaveNotifications { notifications: Vec<PhoneNotification> }
        23 StreamNotifications {}
        24 ListIntegrations {}
        25 CreateIntegration { provider: String, label: String, config: String }
        26 DeleteIntegration { id: String }
        27 UpdateCredentials { id: String, credentials: String, status: String }
    }
}

wire_enum! {
    #[derive(Debug, Clone)]
    pub enum Reply {
        0 Done {}
        1 Error { code: u16, message: String }
        2 Run { run: Run }
        3 Event { seq: i64, event: RunEvent }
        4 Heartbeat {}
        5 Sessions { sessions: Vec<Session> }
        6 History { entries: Vec<HistoryEntry> }
        7 Memories { memories: Vec<Memory> }
        8 Crons { crons: Vec<Cron> }
        9 Created { id: String }
        10 Enabled { enabled: bool }
        11 Resolved { resolved: bool }
        12 Saved { count: u64 }
        13 Integrations { integrations: Vec<Integration> }
        14 Notice { notice: Notice }
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
        let request = Request::GetRun { id: "r1".into() };
        let buf = rmp_serde::to_vec(&request).unwrap();
        // fixarray(2), op 3, fixstr(2) "r1"
        assert_eq!(buf, [0x92, 0x03, 0xa2, b'r', b'1']);
        assert_eq!(rmp_serde::from_slice::<Request>(&buf).unwrap(), request);

        let chat = Request::Chat {
            session_id: Uuid::new_v4(),
            request_id: Uuid::new_v4(),
            message: "hi".into(),
            image: None,
        };
        let buf = rmp_serde::to_vec(&chat).unwrap();
        assert_eq!(rmp_serde::from_slice::<Request>(&buf).unwrap(), chat);
    }

    #[test]
    fn nested_types_round_trip() {
        let reply = Reply::Event {
            seq: 7,
            event: RunEvent::DeviceQuery {
                query_id: "q".into(),
                query: DeviceQuery::Contacts { name: "Ana".into() },
            },
        };
        let buf = rmp_serde::to_vec(&reply).unwrap();
        let decoded: Reply = rmp_serde::from_slice(&buf).unwrap();
        assert_eq!(rmp_serde::to_vec(&decoded).unwrap(), buf);
    }

    #[test]
    fn decode_rejects_bad_requests() {
        assert!(
            rmp_serde::from_slice::<Request>(&[0x91, 0x7f]).is_err(),
            "unknown op"
        );
        assert!(
            rmp_serde::from_slice::<Request>(&[0x91, 0x03]).is_err(),
            "missing argument"
        );
    }
}
