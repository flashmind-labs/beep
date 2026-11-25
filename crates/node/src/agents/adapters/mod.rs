//! The ACP adapters the node installs itself, for agents whose CLI is here
//! without them: a machine with Claude Code or Codex from their native
//! installers and no Node.js. `layout` says where they live, `node_dist`
//! which Node.js to fetch, `install` does it and `installs` keeps at it.

pub(in crate::agents) mod install;
pub(in crate::agents) mod layout;
mod node_dist;

pub(crate) use layout::{Installed, installed};
