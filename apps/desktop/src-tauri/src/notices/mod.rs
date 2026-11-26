//! Notices from the machines of the mesh: routing each one, and the bell. Entry point: `serve`.

mod route;
mod watch;

pub(crate) use route::{answer_agent, follow_actions, serve};
pub(crate) use watch::{Looks, follow, pushed_moves};
