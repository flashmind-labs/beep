//! Voice notes: notices spoken and spoken replies transcribed. Keys and upstream bodies never reach clients.

mod speech;
pub use speech::{Voice, VoiceConfig, VoiceError, spoken_text};

pub mod filler;
pub mod stt;
