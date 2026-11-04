//! Running things on the machine: process groups (`process`), shell
//! sessions (`shell`), background commands (`tasks`) and terminals
//! (`terminal`).

pub(crate) mod lines;
pub(crate) mod process;
pub(crate) mod shell;
pub(crate) mod tasks;
pub(crate) mod terminal;
