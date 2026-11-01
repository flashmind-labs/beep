#![allow(dead_code, reason = "the project agent uses these in the next tasks")]

//! Projects: a folder of work with its own agent, whose personas live in
//! Markdown files next to the code.

mod commands;
pub(crate) mod model;
mod personas;
pub(crate) mod place;
pub(crate) mod policy;
