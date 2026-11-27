//! Notices from the machines of the mesh: routing each one, and the bell. Entry point: `serve`.

mod forge;
mod route;
mod watch;

pub(crate) use forge::follow_reads;
pub(crate) use route::{answer_agent, follow_actions, serve, show_new};
pub(crate) use watch::{Looks, follow, pushed_moves};
