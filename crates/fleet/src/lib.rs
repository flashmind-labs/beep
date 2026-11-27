//! The desktop side of projects, credentials, settings and usage over the mesh.

pub mod alerts;
mod changed;
mod credentials;
mod desk;
mod direct_syncs;
mod error;
mod git_board;
mod issues;
mod left_folder;
mod machines;
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
mod team;
pub mod temporary;
mod usage;
mod usage_cache;

pub use beep_mesh::machine::sessions::MoveState;
pub use beep_proto::node::skill_library::library_name;
pub use changed::{Changed, ProjectRead};
pub use desk::Desk;
pub use error::{FleetError, Result};
pub use git_board::{GitKept, GitUpdate};
pub use issues::{has_project, places, project_id, project_names};
pub use machines::{Machines, Opened};
pub use moves::{ChatMoveAsk, ChatSession, MoveAsk};
pub use nearest::nearest_first;
pub use notice_read::{Read, digest, mark_read, read_marks};
pub use observer::{observe, phone_ticket};
pub use projects::{ImportJob, RenameRepositoryTarget, RepositoryRename, RepositoryRenameResult};
pub use session::{Options, Session};

pub(crate) type AsyncMutex<T> = tokio::sync::Mutex<T>;
