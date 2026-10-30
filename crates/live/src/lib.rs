//! Live mode's decisions, kept apart from audio and the network so they
//! can be tested and shared by every app: spoken commands, what one turn
//! said, the loop of listening, thinking and speaking, how a reply is read
//! out, and how loud the microphone is.

mod command;
mod level;
mod live;
mod reading;
mod turn;

pub use level::pcm16_level;
pub use live::{
    Effect, EndMode, Input, Leaving, Live, PAUSE_SEND, PHRASE_PAUSE, Phase, QUIET_LIMIT,
    SOON_AFTER, Toast,
};
pub use reading::{Filler, Fillers, Reading};
