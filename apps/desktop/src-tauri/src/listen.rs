//! Live mode's decisions, kept apart from audio and the network so they
//! can be tested: spoken commands, what one turn said, the loop of
//! listening, thinking and speaking, and how a reply is read out.

mod command;
mod live;
mod reading;
mod turn;

pub use live::{Effect, EndMode, Input, Leaving, Live, Phase};
pub use reading::{Filler, Fillers, Reading};
