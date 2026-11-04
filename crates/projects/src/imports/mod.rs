//! Importing a repository onto a machine: which repository, where it goes, and the clone itself.

pub mod clone;
mod error;
pub mod flow;
pub mod folder;
pub mod fork;
pub mod found;
pub mod gone;
pub mod host_repos;
pub mod hosts;
pub mod init;
pub mod link;
pub mod machine;
pub mod place;
pub mod preview;
pub mod records;
pub mod repo;
pub mod store;

pub use error::ImportError;
