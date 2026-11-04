//! The project command lines and their parsers live in `beep-projects`; the server's modules
//! reach them through here.

pub(crate) use beep_projects::commands::*;
pub(crate) use beep_projects::{CommandOutput, Shell, quote};
