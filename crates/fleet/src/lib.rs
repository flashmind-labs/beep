//! The desktop side of projects, credentials, settings and usage over the mesh.

pub mod alerts;
mod credentials;
mod desk;
mod error;
mod git_board;
mod issues;
mod left_folder;
mod moves;
mod nearest;
mod notice_read;
mod observer;
mod open_items;
mod peer;
mod projects;
mod records;
mod runs;
mod session;
pub mod session_titles;
mod settings;
mod skill_libraries;
mod sync;
pub mod temporary;
mod usage;

pub use beep_mesh::machine::sessions::{MoveState, WorktreeMove};
pub use desk::Desk;
pub use error::{FleetError, Result};
pub use git_board::{GitKept, GitUpdate};
pub use issues::{has_project, places, project_id, project_names};
pub use moves::{AskedWorktree, MoveAsk};
pub use nearest::nearest_first;
pub use notice_read::{Read, digest, mark_read, read_marks};
pub use observer::{observe, phone_ticket};
pub use peer::Peers;
pub use projects::ImportJob;
pub use session::{Changed, Options, Session};
pub use skill_libraries::library_name;

pub(crate) type AsyncMutex<T> = tokio::sync::Mutex<T>;
