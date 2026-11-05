//! pi: `<sessions>/--<folder>--/<time>_<id>.jsonl`, the folder in the
//! `session` header on the first line as `cwd`.

use super::{
    Stored,
    folder::remap,
    json::{Step, replace, string_at, string_spans},
};
use std::path::{Path, PathBuf};

pub(super) fn rewrite(transcript: &[u8], file_name: &str, old: &Path, new: &Path) -> Stored {
    let mut lines = transcript.split_inclusive(|b| *b == b'\n');
    let mut bytes = Vec::with_capacity(transcript.len() + 64);
    if let Some(header) = lines.next() {
        if string_at(header, &[Step::Key("type")]).as_deref() == Some("session") {
            let spans = string_spans(header, &[Step::Key("cwd")]);
            bytes.extend(replace(header, &spans, |cwd| remap(cwd, old, new)));
        } else {
            bytes.extend_from_slice(header);
        }
    }
    for line in lines {
        bytes.extend_from_slice(line);
    }
    Stored {
        bytes,
        path: PathBuf::from(folder_name(new)).join(file_name),
    }
}

/// `--home-user-proj--` for `/home/user/proj`.
fn folder_name(folder: &Path) -> String {
    let text = folder.to_string_lossy();
    let text: String = text
        .trim_start_matches(['/', '\\'])
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':') {
                '-'
            } else {
                c
            }
        })
        .collect();
    format!("--{text}--")
}
