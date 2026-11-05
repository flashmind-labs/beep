//! Calls to GitHub and Forgejo for a user's open items, forks and new repositories.

pub mod access;
pub mod board;
pub mod forgejo;
pub mod forks;
pub mod github;
pub mod host;
pub mod new_repo;

pub use host::{Caller, HostError, send_error, status_error};
