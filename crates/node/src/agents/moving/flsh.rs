//! flsh (0.2.9 on): `<data dir>/sessions/<folder hash>/<sha256 of id>.jsonl`,
//! the folder in the `session_metadata` header on the first line as
//! `metadata.working_dir`. The folder hash is flsh's own: the std
//! `DefaultHasher` over the session's `Path`, as 16 hex digits.

use super::{
    folder::Folders,
    harness::Stored,
    json::{Step, replace, string_spans},
};
use crate::{agents::AgentKind, machine::env::output};
use anyhow::{Result, bail};
use beep_proto::SessionId;
use beep_proto::node::harness::{is_newer, parse_version};
use sha2::{Digest, Sha256};
use std::{
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
};

/// The first flsh that keeps its sessions in JSONL files.
const FIRST_JSONL: &str = "0.2.9";

pub(super) fn rewrite(transcript: &[u8], id: &SessionId, folders: &Folders) -> Stored {
    let mut lines = transcript.split_inclusive(|b| *b == b'\n');
    let mut bytes = Vec::with_capacity(transcript.len() + 64);

    if let Some(header) = lines.next() {
        let spans = string_spans(header, &[Step::Key("metadata"), Step::Key("working_dir")]);
        bytes.extend(replace(header, &spans, |dir| folders.remap(dir)));
    }

    for line in lines {
        bytes.extend_from_slice(line);
    }
    Stored {
        bytes,
        path: PathBuf::from(folder_hash(&folders.new)).join(file_name(id.as_ref())),
    }
}

/// The folder flsh keeps the sessions it runs in `folder` in.
fn folder_hash(folder: &Path) -> String {
    let mut hasher = DefaultHasher::new();
    folder.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// The name flsh gives session `id`'s file; flsh refuses a folder holding
/// a file whose name does not match its session.
fn file_name(id: &str) -> String {
    format!("{}.jsonl", hex::encode(Sha256::digest(id.as_bytes())))
}

/// Refuses a move to or from a flsh too old to keep its sessions in files.
/// A version that cannot be read is let through: resuming says what is
/// wrong.
pub(super) async fn check(kind: &AgentKind, machine: &str) -> Result<()> {
    let version = output(&kind.program, &["--version"])
        .await
        .as_deref()
        .and_then(parse_version);
    match version {
        Some(version) if is_newer(FIRST_JSONL, &version) => bail!(outdated(machine)),
        _ => Ok(()),
    }
}

/// Why a session of a flsh too old on `machine` cannot move.
pub(super) fn outdated(machine: &str) -> String {
    format!("Update flsh on {machine} to {FIRST_JSONL} to move this session")
}
