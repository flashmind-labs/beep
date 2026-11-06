//! How the apps reach beep-server: HTTP, behind a proxy such as Cloudflare
//! that holds TLS.
//!
//! Each request is `POST /v1/<Request>` (`path`) with the request as its
//! msgpack body. Every message is encoded with field names (`encode`), so a
//! struct gains a field without breaking a reader that does not know it.
//!
//! - A `Call` answers 200 with the reply as the body, or an error status
//!   (`status`) with an `ApiError` body.
//! - A `Stream` answers 200 with frames: each a four-byte big-endian length,
//!   then one tag byte (`OK` or `ERR`) and a msgpack body. The body ends when
//!   the stream does; an `ERR` frame is its last.
//! - A `Duplex` sends its request body as frames too, untagged: the request
//!   first, then each `Up`.
//!
//! Signing in: the app signs `session_message` with its device key and posts
//! it to `SESSION_PATH`, which answers a `SessionToken` good for
//! `SESSION_TTL`. Every other request sends it as `Authorization: Bearer`. A
//! 401 means the token is missing or stale: get a new one and send again. The
//! key is checked against the account on every request, so a removed key is
//! refused at once (403 with `REFUSED_HEADER`).
//!
//! `EVENTS_PATH` is one long `GET` of untagged `AppEvent` frames: what the
//! user's devices are told, a `Heartbeat` every `HEARTBEAT` so proxies keep it
//! open, and `?after=<id>` resumes after the last `AppEvent::Notice` read.

use crate::{ApiError, Notice, Op, Platform, SecretText};
use bytes::{BufMut, Bytes, BytesMut};
use iroh::EndpointId;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::time::Duration;
use tokio_util::codec::LengthDelimitedCodec;

pub const PREFIX: &str = "/v1/";
/// `GET`: `ServerInfo`, with no sign-in.
pub const SERVER_PATH: &str = "/v1/server";
/// `POST` a `SessionStart`: a `SessionToken`.
pub const SESSION_PATH: &str = "/v1/session";
/// `GET`, signed in: the `AppEvent` stream.
pub const EVENTS_PATH: &str = "/v1/events";
pub const CONTENT_TYPE: &str = "application/msgpack";
/// Which app sends the request (`Platform::as_header`).
pub const PLATFORM_HEADER: &str = "beep-platform";
/// The app's own version, such as "0.1.0 (142)".
pub const VERSION_HEADER: &str = "beep-version";
/// On a 403 for the key itself: `Refusal::as_str`.
pub const REFUSED_HEADER: &str = "beep-refused";

/// How long a `SessionToken` is good for.
pub const SESSION_TTL: Duration = Duration::from_secs(60 * 60);
/// How far a `SessionStart`'s clock may be from the server's.
pub const SESSION_SKEW: Duration = Duration::from_secs(5 * 60);
/// The longest the events stream stays quiet; Cloudflare closes a stream
/// idle for about 100 seconds.
pub const HEARTBEAT: Duration = Duration::from_secs(30);
/// The largest frame or body either side takes.
pub const MAX_FRAME: usize = 8 * 1024 * 1024;

/// Stream frame tags.
pub const OK: u8 = 0;
pub const ERR: u8 = 1;

const SESSION_CONTEXT: &[u8] = b"beep-session/1\0";

/// The path of request `op`, such as `/v1/ListSessions`.
pub fn path(op: Op) -> String {
    format!("{PREFIX}{op:?}")
}

/// `value` as msgpack with field names.
pub fn encode<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, rmp_serde::encode::Error> {
    rmp_serde::to_vec_named(value)
}

pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, rmp_serde::decode::Error> {
    rmp_serde::from_slice(bytes)
}

/// Frames as both sides cut them: a four-byte length, then the frame.
pub fn codec() -> LengthDelimitedCodec {
    LengthDelimitedCodec::builder()
        .max_frame_length(MAX_FRAME)
        .new_codec()
}

/// `body` with its length in front, as `codec` reads it.
pub fn length_prefixed(body: &[u8]) -> Bytes {
    let mut out = BytesMut::with_capacity(4 + body.len());
    // A frame never reaches 4 GiB: `MAX_FRAME` refuses it long before.
    out.put_u32(u32::try_from(body.len()).unwrap_or(u32::MAX));
    out.put_slice(body);
    out.freeze()
}

/// One stream frame: `tag`, then `body`, length first.
pub fn tagged(tag: u8, body: &[u8]) -> Bytes {
    let mut framed = Vec::with_capacity(1 + body.len());
    framed.push(tag);
    framed.extend_from_slice(body);
    length_prefixed(&framed)
}

/// The HTTP status an `ApiError` answers with.
pub fn status(error: &ApiError) -> u16 {
    match error {
        ApiError::BadRequest { .. } => 400,
        ApiError::Unauthorized {} => 403,
        ApiError::NotFound {} => 404,
        ApiError::Conflict { .. } => 409,
        ApiError::Internal {} => 500,
    }
}

impl Platform {
    pub fn as_header(self) -> &'static str {
        match self {
            Self::Android {} => "android",
            Self::Ios {} => "ios",
            Self::Macos {} => "macos",
        }
    }

    pub fn from_header(text: &str) -> Option<Self> {
        match text {
            "android" => Some(Self::Android {}),
            "ios" => Some(Self::Ios {}),
            "macos" => Some(Self::Macos {}),
            _ => None,
        }
    }
}

/// Why the server refuses the key itself, not one request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    /// The account removed this key.
    #[error("this device was removed from the account")]
    Revoked,
    /// A single-user server's key that is not one of its owner's. The owner
    /// links more devices with `LinkDevice`.
    #[error("this server is someone else's")]
    NotInvited,
}

impl Refusal {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Revoked => "revoked",
            Self::NotInvited => "not-invited",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "revoked" => Some(Self::Revoked),
            "not-invited" => Some(Self::NotInvited),
            _ => None,
        }
    }
}

/// What `SERVER_PATH` says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerInfo {
    /// The server's key, as link offers name it.
    pub id: String,
    /// Whether the server has hosted sign-in: a key it does not know is a
    /// guest that signs in through the website, and connecting makes no
    /// account.
    pub login: bool,
}

/// Asks for a session token: `signature` is the device key's over
/// `session_message`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionStart {
    /// The device's key.
    pub key: String,
    /// Seconds since the Unix epoch.
    pub at: u64,
    #[serde(with = "serde_bytes")]
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionToken {
    pub token: SecretText,
    /// Seconds from now.
    pub expires_in: u64,
}

/// What a device signs to get a session token from `server`.
pub fn session_message(server: &EndpointId, key: &EndpointId, at: u64) -> Vec<u8> {
    let mut message = SESSION_CONTEXT.to_vec();
    message.extend_from_slice(server.as_bytes());
    message.extend_from_slice(key.as_bytes());
    message.extend_from_slice(&at.to_be_bytes());
    message
}

wire_enum! {
    /// One frame of the events stream:
    ///
    /// - `Resume`, first on each stream: the notice this one follows on
    ///   from, for `?after=` on the next, even when no notice came.
    /// - `Updated`, second and whenever it moves: the user's newest update
    ///   id; call `GetUpdates` from the last one read.
    /// - `Notice`, one for this device. `id` grows. Boxed, as the other
    ///   events are a few bytes.
    /// - `Heartbeat`: every `HEARTBEAT`, so a proxy keeps the stream open.
    #[derive(Debug, Clone, PartialEq)]
    pub enum AppEvent {
        0 Notice { id: i64, notice: Box<Notice> }
        1 Updated { last: i64 }
        2 Heartbeat {}
        3 Resume { after: i64 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh::SecretKey;

    #[test]
    fn a_request_path_is_its_name() {
        assert_eq!(path(Op::ListSessions), "/v1/ListSessions");
    }

    #[test]
    fn a_struct_encodes_with_its_field_names_and_reads_without_new_ones() {
        #[derive(Serialize)]
        struct Newer {
            a: u8,
            b: u8,
        }
        #[derive(Deserialize, Debug, PartialEq)]
        struct Older {
            a: u8,
        }
        let bytes = encode(&Newer { a: 1, b: 2 }).unwrap();
        assert_eq!(decode::<Older>(&bytes).unwrap(), Older { a: 1 });
    }

    #[test]
    fn a_session_message_names_server_key_and_time() {
        let server = SecretKey::from_bytes(&[1; 32]).public();
        let key = SecretKey::from_bytes(&[2; 32]).public();
        let one = session_message(&server, &key, 7);
        assert_ne!(one, session_message(&key, &server, 7));
        assert_ne!(one, session_message(&server, &key, 8));
    }

    #[test]
    fn a_tagged_frame_reads_back_through_the_codec() {
        use tokio_util::codec::Decoder;
        let mut buf = BytesMut::from(&tagged(OK, b"hi")[..]);
        let frame = codec().decode(&mut buf).unwrap().unwrap();
        assert_eq!(&frame[..], &[OK, b'h', b'i']);
    }
}
