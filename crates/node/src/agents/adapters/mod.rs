//! The ACP adapters the node installs itself, for agents whose CLI is here
//! without them: a machine with Claude Code or Codex from their native
//! installers and no Node.js. `layout` says where they live, `node_dist`
//! which Node.js to fetch, `install` does it and `worker` keeps at it.

mod install;
mod layout;
mod node_dist;
mod worker;

pub(crate) use layout::{Installed, installed};
pub(super) use worker::Installs;
