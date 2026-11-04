//! The command center's Cmd+K search: ranks actions, settings, projects,
//! coding-agent sessions, chats, memories, routines and machines against
//! what was typed.

pub(crate) mod lists;
mod projects;
pub(crate) mod rank;

pub(crate) use lists::PaletteCache;
