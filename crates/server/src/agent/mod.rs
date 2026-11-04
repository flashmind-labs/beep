//! The machinery around one agent turn: models and metering, context compaction, checks on the
//! reply (gates, repeats), titles, follow-ups and background sub-agents. `app::App` drives turns.

pub mod cache_session;
pub mod compaction;
pub mod decision;
pub mod followups;
pub mod gates;
pub mod images;
pub mod live;
pub mod metering;
pub mod models;
pub mod paging;
pub mod repeat;
pub mod sources;
pub mod steps;
pub mod subagents;
pub mod timing;
pub mod titles;
pub mod tool_attachments;
pub mod tool_cost;
