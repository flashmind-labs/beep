//! Telling a deleted project folder from one that is only out of reach.
//! A machine's folder of a project counts as gone only when the machine
//! answers, in time, that nothing is at its path while the folder above it
//! is there and holds other things. An offline machine, a look that timed
//! out, a disk that is not mounted (its mount point is empty, missing, or
//! sits right under `/Volumes`, `/mnt` or `/media`) or a folder it cannot
//! read all leave the project as it is.

use super::clone::quote;
use crate::store::{ImportRow, SyncRow};
use std::collections::BTreeSet;

/// What the script prints before each line it means.
const MARK: &str = "BEEP_GONE:";

/// One folder of a project on the machine being looked through.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Place {
    pub(crate) project: String,
    pub(crate) folder: String,
}

/// The project folders `endpoint` has, imported or synced, each once.
/// Folders that are not absolute POSIX paths are left out: the script
/// cannot read them.
pub(crate) fn places(imports: &[ImportRow], syncs: &[SyncRow], endpoint: &str) -> Vec<Place> {
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

/// The script that looks for each of `places` and prints, by its number,
/// `there`, `missing` or `unsure`.
pub(crate) fn script(places: &[Place]) -> String {
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

/// The numbers of the places the script found missing, from its output.
/// Nothing counts unless the script ran to the end in time.
pub(crate) fn missing(output: &str, finished: bool, count: usize) -> Vec<usize> {
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
mod tests {
    use super::*;
    use chrono::Utc;
    use std::{fs, path::Path, process::Command};

    fn import(project: &str, endpoint: &str, folder: &str) -> ImportRow {
        ImportRow {
            project: project.into(),
            endpoint: endpoint.into(),
            folder: folder.into(),
            url: format!("https://github.com/{project}"),
            imported_at: Utc::now(),
        }
    }

    fn place(folder: &Path) -> Place {
        Place {
            project: "acme/widget".into(),
            folder: folder.to_string_lossy().into_owned(),
        }
    }

    fn run(script: &str) -> String {
        let out = Command::new("sh")
            .arg("-c")
            .arg(script)
            .output()
            .expect("sh runs");
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    #[test]
    fn a_machine_s_imported_and_synced_folders_are_looked_at_once() {
        let imports = [
            import("acme/widget", "build", "/srv/widget"),
            import("acme/widget", "laptop", "/home/user/widget"),
            import("acme/site", "build", r"C:\site"),
        ];
        let syncs = [
            SyncRow {
                project: "acme/widget".into(),
                endpoint: "build".into(),
                folder: "/srv/widget".into(),
            },
            SyncRow {
                project: "game".into(),
                endpoint: "build".into(),
                folder: "/srv/game".into(),
            },
        ];
        let folders: Vec<String> = places(&imports, &syncs, "build")
            .into_iter()
            .map(|place| place.folder)
            .collect();
        assert_eq!(folders, ["/srv/widget", "/srv/game"]);
    }

    #[test]
    fn only_a_deleted_folder_beside_others_counts_as_missing() {
        let root = tempfile::tempdir().expect("a temp folder");
        let code = root.path().join("code");
        fs::create_dir_all(code.join("kept")).expect("folders");
        fs::create_dir_all(code.join("other")).expect("folders");
        // A mount point with nothing mounted on it.
        let mount = root.path().join("disk");
        fs::create_dir_all(&mount).expect("folders");
        let places = [
            place(&code.join("kept")),
            place(&code.join("deleted")),
            place(&mount.join("project")),
            place(&root.path().join("nowhere/project")),
            place(Path::new("/mnt/project")),
            place(Path::new("/media/user/project")),
            place(&code.join("it's here")),
        ];
        fs::create_dir_all(code.join("it's here")).expect("folders");
        let output = run(&script(&places));
        assert_eq!(missing(&output, true, places.len()), [1], "{output}");
        // A look that did not finish says nothing.
        assert!(missing(&output, false, places.len()).is_empty());
    }

    #[test]
    fn stray_or_out_of_range_lines_do_not_count() {
        let output = "BEEP_GONE: missing 7\nmissing 0\nBEEP_GONE: missing x\n\
                      BEEP_GONE: unsure 1\nBEEP_GONE: missing 2\r\n";
        assert_eq!(missing(output, true, 3), [2]);
    }
}
