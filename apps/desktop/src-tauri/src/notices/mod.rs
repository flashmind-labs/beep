//! Notices from the machines of the mesh: routing each one, macOS
//! notifications, and the bell's history. Entry point: `serve`.

pub(crate) mod history;
pub(crate) mod notify;
mod route;
mod watch;

pub(crate) use route::{answer_agent, follow_actions, serve};
pub(crate) use watch::{Looks, follow, heard};
