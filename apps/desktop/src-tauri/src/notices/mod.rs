//! Notices the server pushes: routing each one, macOS notifications, and
//! the questions background work asks. Entry point: `serve`.

pub(crate) mod notify;
mod route;
pub(crate) mod work;

pub(crate) use route::serve;
