//! Running things on the machine: process groups (`process`), shell
//! sessions (`shell`), background commands (`tasks`) and terminals
//! (`terminal`), and the emulators and simulators shown on desktops
//! (`device`).

pub(crate) mod device;
pub(crate) mod lines;
pub(crate) mod local_socket;
pub(crate) mod process;
pub(crate) mod shell;
pub(crate) mod tasks;
pub(crate) mod terminal;
