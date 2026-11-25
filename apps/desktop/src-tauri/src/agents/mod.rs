//! Coding agents on the user's machines: their sessions, files, diffs and
//! artifacts, harnesses, sign-ins, custom agents and instructions.

pub(crate) mod accounts;
pub(crate) mod artifacts;
mod child_reply;
pub(crate) mod custom;
pub(crate) mod diff;
pub(crate) mod file_cache;
pub(crate) mod files;
pub(crate) mod files_live;
pub(crate) mod flsh_settings;
pub(crate) mod follow;
pub(crate) mod harnesses;
pub(crate) mod instructions;
pub(crate) mod kept_folder;
pub(crate) mod kept_transcripts;
pub(crate) mod local_file;
pub(crate) mod media;
pub(crate) mod moving;
pub(crate) mod outbox;
pub(crate) mod processes;
pub(crate) mod review;
pub(crate) mod sessions;
pub(crate) mod starts;
pub(crate) mod stash;
pub(crate) mod steer;
pub(crate) mod work_copy;
