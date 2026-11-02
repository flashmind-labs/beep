//! Things that happen in the user's connected accounts wake the agent, which
//! decides whether to tell the user and what to offer to do.
//!
//! Events come two ways. Composio triggers (new Gmail and Outlook mail,
//! Google and Outlook calendar changes) arrive over Composio's realtime
//! channel, a Pusher websocket the server opens itself, so no public webhook
//! is needed (see [`realtime`]). What has no trigger is polled: GitHub
//! notifications, Fastmail, and Gmail or Outlook while their trigger is not
//! set up (see [`poll`]). Both become one [`AppEvent`] shape and go through
//! one pipeline (see [`pipeline`]): dropped when the user turned the topic
//! off, stored once per happening, rate limited per user, batched for a
//! moment and handed to a triage sub-agent (see [`triage`]). Only what it
//! passes on as urgent wakes the main agent, which decides what to tell the
//! user; what can wait comes up in the next wake or conversation.
//!
//! Every user gets them.

pub(crate) mod limit;
mod model;
pub mod normalize;
pub(crate) mod pipeline;
mod poll;
pub(crate) mod realtime;
pub(crate) mod tools;
pub(crate) mod triage;
pub(crate) mod triggers;

pub use limit::Limits;
pub use model::{AppEvent, DedupeKey, EventApp, EventKind, Source, Topic};
pub use pipeline::{Accepted, Hub, Settings, accept, start, submit};
pub use realtime::deliver;
pub use triage::{EventsConfig, Urgency, Verdict, waiting_updates};
