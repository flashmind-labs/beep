//! Telling a deleted project folder from an unreachable one. A folder is gone only when the machine
//! answers in time that its path is empty while the parent exists and holds other things;
//! an unmounted disk (empty, missing, or right under `/Volumes`, `/mnt` or `/media`) never counts.

use super::records::{ImportRow, SyncRow};
use crate::quote;
use std::collections::BTreeSet;

/// What the script prints before each line it means.
const MARK: &str = "BEEP_GONE:";

/// One folder of a project on the machine being looked through.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Place {
    pub project: String,
    pub folder: String,
}

/// The project folders on `endpoint`, imported or synced, each once; non-absolute paths are skipped.
pub fn places(imports: &[ImportRow], syncs: &[SyncRow], endpoint: &str) -> Vec<Place> {
    let imported = imports
        .iter()
        .filter(|row| row.endpoint == endpoint)
        .map(|row| (&row.project, &row.folder));
    let synced = syncs
        .iter()
        .filter(|row| row.endpoint == endpoint)
        .map(|row| (&row.project, &row.folder));
    imported
        .chain(synced)
        .filter(|(_, folder)| folder.starts_with('/') && !folder.contains(['\n', '\r', '\0']))
        .map(|(project, folder)| Place {
            project: project.clone(),
            folder: folder.clone(),
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// The script printing `there`, `missing` or `unsure` for each of `places` by number.
pub fn script(places: &[Place]) -> String {
    let mut out = format!(
        "look() {{\n\
         \x20 if [ -e \"$1\" ] || [ -L \"$1\" ]; then printf '{MARK} there %s\\n' \"$2\"; return; fi\n\
         \x20 up=$(dirname -- \"$1\")\n\
         \x20 case $up in /|/Volumes|/mnt|/media|/run/media) printf '{MARK} unsure %s\\n' \"$2\"; return;; esac\n\
         \x20 case $(dirname -- \"$up\") in /media|/run/media) printf '{MARK} unsure %s\\n' \"$2\"; return;; esac\n\
         \x20 if [ -d \"$up\" ] && [ -n \"$(ls -A -- \"$up\" 2>/dev/null)\" ]; then\n\
         \x20   printf '{MARK} missing %s\\n' \"$2\"\n\
         \x20 else\n\
         \x20   printf '{MARK} unsure %s\\n' \"$2\"\n\
         \x20 fi\n\
         }}\n"
    );
    for (n, place) in places.iter().enumerate() {
        out.push_str(&format!("look {} {n}\n", quote(&place.folder)));
    }
    out
}

/// The numbers of the places found missing; nothing counts unless the script finished in time.
pub fn missing(output: &str, finished: bool, count: usize) -> Vec<usize> {
    if !finished {
        return Vec::new();
    }
    let mut out = BTreeSet::new();
    for line in output.lines() {
        let Some(said) = line.trim_end_matches('\r').strip_prefix(MARK) else {
            continue;
        };
        let mut words = said.split_whitespace();
        if words.next() != Some("missing") {
            continue;
        }
        if let Some(n) = words.next().and_then(|n| n.parse::<usize>().ok())
            && n < count
        {
            out.insert(n);
        }
    }
    out.into_iter().collect()
}

#[cfg(test)]
#[path = "../tests/imports_gone.rs"]
mod tests;
