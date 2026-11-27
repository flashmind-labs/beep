//! Claude Code: `<config>/projects/<folder slug>/<id>.jsonl`, the folder
//! on every line as `cwd`.

use super::{
    Stored,
    folder::Folders,
    json::{Step, replace, string_spans},
};
use crate::logs::claude_dir::slug;
use beep_proto::SessionId;
use std::path::PathBuf;

pub(super) fn rewrite(transcript: &[u8], id: &SessionId, folders: &Folders) -> Stored {
    let mut bytes = Vec::with_capacity(transcript.len() + 64);

    for line in transcript.split_inclusive(|b| *b == b'\n') {
        let spans = string_spans(line, &[Step::Key("cwd")]);
        bytes.extend(replace(line, &spans, |cwd| folders.remap(cwd)));
    }
    Stored {
        bytes,
        path: PathBuf::from(slug(&folders.new)).join(format!("{id}.jsonl")),
    }
}
