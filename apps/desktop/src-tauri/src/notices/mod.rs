//! Notices the server pushes: routing each one, macOS notifications, the
//! questions background work asks, and the bell's history from the mesh.
//! Entry point: `serve`.

pub(crate) mod alerts;
pub(crate) mod history;
pub(crate) mod notify;
mod route;
mod watch;
pub(crate) mod work;

pub(crate) use route::serve;
pub(crate) use watch::follow;
