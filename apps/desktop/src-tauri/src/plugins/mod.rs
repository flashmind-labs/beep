//! Plugins: the host that runs the folders under the plugins directory, and
//! the commands the page uses to read what they placed, learn which topics they
//! follow and pass updates in. A phone runs none.

pub(crate) mod command;
#[cfg(desktop)]
mod host;

#[cfg(desktop)]
pub(crate) use host::start;
