//! The desktop side of projects, credentials, settings and usage over the mesh.

mod credentials;
mod desk;
mod error;
mod issues;
pub mod migrate;
mod peer;
mod projects;
mod records;
mod runs;
mod session;
mod settings;
mod sync;
mod usage;

pub use desk::Desk;
pub use error::{FleetError, Result};
pub use issues::{places, project_id};
pub use peer::Peers;
pub use projects::ImportJob;
pub use session::{Changed, Options, Session};

pub(crate) type AsyncMutex<T> = tokio::sync::Mutex<T>;
