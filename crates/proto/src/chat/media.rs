//! Files, images and voice sent with a message.

use crate::{Duplex, Transcribe};
use serde::{Deserialize, Serialize};

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
