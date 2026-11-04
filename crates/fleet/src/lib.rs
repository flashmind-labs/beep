//! The desktop side of projects, credentials, settings and usage over the mesh.

mod credentials;
mod desk;
mod error;
mod issues;
mod peer;
mod projects;
mod records;
mod settings;
mod sync;
mod usage;

pub use desk::Desk;
pub use error::{FleetError, Result};
pub use issues::project_id;
pub use peer::Peers;
pub use projects::ImportJob;

pub(crate) type AsyncMutex<T> = tokio::sync::Mutex<T>;
