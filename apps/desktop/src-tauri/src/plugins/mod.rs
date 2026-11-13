//! Plugins: the host that runs the folders under the plugins directory, and
//! the command the page reads what they placed with. A phone runs none.

pub(crate) mod command;
#[cfg(desktop)]
mod host;

#[cfg(desktop)]
pub(crate) use host::start;
