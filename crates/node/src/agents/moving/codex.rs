//! Codex: `<home>/sessions/<yyyy>/<mm>/<dd>/rollout-<time>-<id>.jsonl`. The
//! folder is in the `session_meta` line (`cwd`, `runtime_workspace_roots`)
//! and in each `turn_context` line (`cwd`). Codex's own schema types them as
//! plain path strings (`runtimeWorkspaceRoots` is a list of them); a `file://`
//! URI would be moved too, should one turn up.

use super::{
    Stored,
    folder::remap,
    json::{Step, replace, string_at, string_spans},
};
use std::path::{Path, PathBuf};

pub(super) fn rewrite(transcript: &[u8], file_name: &str, old: &Path, new: &Path) -> Stored {
    let mut bytes = Vec::with_capacity(transcript.len() + 64);
    for line in transcript.split_inclusive(|b| *b == b'\n') {
        let mut spans = match string_at(line, &[Step::Key("type")]).as_deref() {
            Some("session_meta") => {
                let mut spans = string_spans(line, &[Step::Key("payload"), Step::Key("cwd")]);
                spans.extend(string_spans(
                    line,
                    &[
                        Step::Key("payload"),
                        Step::Key("runtime_workspace_roots"),
                        Step::Each,
                    ],
                ));
                spans
            }
            Some("turn_context") => string_spans(line, &[Step::Key("payload"), Step::Key("cwd")]),
            _ => Vec::new(),
        };
        spans.sort_by_key(|span| span.start);
        bytes.extend(replace(line, &spans, |folder| remap(folder, old, new)));
    }
    Stored {
        bytes,
        path: day_folder(file_name).join(file_name),
    }
}

/// `sessions/<yyyy>/<mm>/<dd>` from `rollout-<yyyy>-<mm>-<dd>T...`.
fn day_folder(file_name: &str) -> PathBuf {
    let day = file_name.strip_prefix("rollout-").unwrap_or(file_name);
    let mut parts = day.splitn(3, '-');
    let (year, month, rest) = (parts.next(), parts.next(), parts.next());
    let date = rest.and_then(|rest| rest.get(..2));
    match (year, month, date) {
        (Some(y), Some(m), Some(d)) => PathBuf::from("sessions").join(y).join(m).join(d),
        _ => PathBuf::from("sessions"),
    }
}
