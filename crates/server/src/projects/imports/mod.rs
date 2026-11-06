//! The user's imported projects as the server recorded them. Importing, cloning and looking for
//! checkouts need a machine, which the server does not reach: a desktop does them over its mesh.

mod remove;
mod service;

pub(crate) use remove::{machine_removed, remove_project};
pub(crate) use service::{imported, project_machines};
