//! Claude Code: `<config>/projects/<folder slug>/<id>.jsonl`, the folder
//! on every line as `cwd`.

use super::{
    Stored,
    folder::remap,
    json::{Step, replace, string_spans},
};
use crate::agents::claude::slug;
use std::path::{Path, PathBuf};

pub(super) fn rewrite(transcript: &[u8], id: &str, old: &Path, new: &Path) -> Stored {
    let mut bytes = Vec::with_capacity(transcript.len() + 64);
    for line in transcript.split_inclusive(|b| *b == b'\n') {
        let spans = string_spans(line, &[Step::Key("cwd")]);
        bytes.extend(replace(line, &spans, |cwd| remap(cwd, old, new)));
    }
    Stored {
        bytes,
        path: PathBuf::from(slug(new)).join(format!("{id}.jsonl")),
    }
}
