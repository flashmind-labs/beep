//! Importing a repository onto a machine: which repository, where it goes, and the clone itself.

pub mod clone;
mod error;
pub mod folder;
pub mod place;
pub mod repo;

pub use error::ImportError;
