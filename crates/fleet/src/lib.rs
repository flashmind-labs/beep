//! The desktop side of projects, credentials, settings and usage over the mesh.

mod credentials;
mod desk;
mod error;
mod git_board;
mod issues;
mod left_folder;
pub mod migrate;
mod moves;
mod nearest;
mod open_items;
mod peer;
mod projects;
mod records;
mod runs;
mod session;
mod settings;
mod sync;
pub mod temporary;
mod usage;

pub use beep_mesh::machine::sessions::{MoveState, WorktreeMove};
pub use beep_proto::node::agent::CopyView;
pub use desk::Desk;
pub use error::{FleetError, Result};
pub use git_board::{GitKept, GitUpdate};
pub use issues::{has_project, places, project_id, project_names};
pub use moves::{AskedWorktree, CopyState, MoveAsk};
pub use nearest::nearest_first;
pub use peer::Peers;
pub use projects::ImportJob;
pub use session::{Changed, Options, Session};

pub(crate) type AsyncMutex<T> = tokio::sync::Mutex<T>;
