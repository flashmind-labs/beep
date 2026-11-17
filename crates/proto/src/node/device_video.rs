//! `device-video/1`: native H.264 Annex B bytes, in order, over one mesh
//! stream. No encoded bytes may be skipped. Clients without a video decoder
//! and peers without this protocol keep using `device/1` JPEG frames.

use super::device::{DeviceId, Input};
use serde::{Deserialize, Serialize};

pub const PROTOCOL: &str = "device-video/1";
pub const FPS: u32 = 60;
/// Native encoder chunks are split to keep transport and IPC buffers bounded.
pub const MAX_CHUNK: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Request {
    Attach { id: DeviceId, max_side: u32 },
    Input { input: Input },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Rotation {
    Upright,
    Left,
    UpsideDown,
    Right,
}

impl From<Rotation> for u32 {
    fn from(rotation: Rotation) -> Self {
        match rotation {
            Rotation::Upright => 0,
            Rotation::Left => 1,
            Rotation::UpsideDown => 2,
            Rotation::Right => 3,
        }
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub enum Reply {
    /// A fresh encoder. The client must discard its previous decoder state.
    Started {
        fps: u32,
    },
    Data {
        #[serde(with = "serde_bytes")]
        data: Vec<u8>,
        rotation: Rotation,
    },
    Unsupported {
        reason: String,
    },
    Ended {
        reason: String,
    },
}

impl std::fmt::Debug for Reply {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Started { fps } => f.debug_struct("Started").field("fps", fps).finish(),
            Self::Data { data, rotation } => f
                .debug_struct("Data")
                .field("bytes", &data.len())
                .field("rotation", rotation)
                .finish(),
            Self::Unsupported { reason } => f
                .debug_struct("Unsupported")
                .field("reason", reason)
                .finish(),
            Self::Ended { reason } => f.debug_struct("Ended").field("reason", reason).finish(),
        }
    }
}
