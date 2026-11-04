//! Account events wake the agent, which decides whether to tell the user.
//!
//! Composio triggers arrive over its realtime channel ([`realtime`]); the rest is polled
//! ([`poll`]). Both become [`AppEvent`]s through one [`pipeline`]: filtered by topic, stored
//! once, rate limited, batched and triaged ([`triage`]). Only urgent ones wake the main agent.

pub(crate) mod limit;
mod model;
pub mod normalize;
pub(crate) mod pipeline;
mod poll;
pub(crate) mod realtime;
pub(crate) mod tools;
pub(crate) mod triage;
pub(crate) mod triggers;
pub(crate) mod update_chat;

pub use limit::Limits;
pub use model::{AppEvent, DedupeKey, EventApp, EventKind, Source, Topic};
pub use pipeline::{Accepted, Hub, Settings, accept, start, submit};
pub use realtime::deliver;
pub use triage::{EventsConfig, Urgency, Verdict, waiting_updates};
