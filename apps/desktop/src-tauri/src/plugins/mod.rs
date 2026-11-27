//! Plugins: the host that runs the folders under the plugins directory, and
//! the commands the page uses to read what they placed, learn which topics they
//! follow, report which slots are on the page, serve their images and status, and pass
//! updates, control events and window visibility in. It also installs, updates, allows,
//! turns off and removes plugins for the Settings page. A phone runs none.

pub(crate) mod command;
#[cfg(desktop)]
mod git;
#[cfg(desktop)]
mod host;
#[cfg(desktop)]
mod install;
#[cfg(desktop)]
pub(crate) mod manage;
#[cfg(desktop)]
mod topics;

#[cfg(desktop)]
pub(crate) use host::start;
#[cfg(desktop)]
pub(crate) use install::Installer;
#[cfg(desktop)]
pub(crate) use topics::follow as follow_topics;
