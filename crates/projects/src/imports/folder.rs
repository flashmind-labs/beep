//! Folders the user picks on a machine, read through the node's shell so any readable folder works.

use super::ImportError;
use crate::{Shell, quote};
use beep_proto::MachineFolder;
use std::time::Duration;

/// What the listing script prints before each line it means.
const MARK: &str = "BEEP_FOLDER:";
/// The most folders one listing names.
const MAX_FOLDERS: u32 = 500;
/// The longest path taken.
const MAX_PATH: usize = 1024;

/// A folder on a machine as given: absolute, or under `~`, which the machine's shell expands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderPath {
    /// `/` and the parts after it, without a trailing `/`.
    Absolute(String),
    /// The parts after `~/`; empty for the home folder itself.
    Home(String),
}

impl FolderPath {
    /// The folder `input` names, `None` when blank; refuses relative paths, `..` and control characters.
    pub fn parse(input: &str) -> Result<Option<Self>, ImportError> {
        let input = input.trim();

        if input.is_empty() {
            return Ok(None);
        }

        if input.len() > MAX_PATH || input.chars().any(char::is_control) {
            return Err(ImportError::BadFolder);
        }

        let (home, rest) = if input == "~" {
            (true, "")
        } else if let Some(rest) = input.strip_prefix("~/") {
            (true, rest)
        } else if let Some(rest) = input.strip_prefix('/') {
            (false, rest)
        } else {
            return Err(ImportError::BadFolder);
        };
        let mut parts = Vec::new();

        for part in rest.split('/') {
            match part {
                "" | "." => {}
                ".." => return Err(ImportError::BadFolder),
                part => parts.push(part),
            }
        }

        let joined = parts.join("/");
        Ok(Some(if home {
            Self::Home(joined)
        } else {
            Self::Absolute(format!("/{joined}"))
        }))
    }

    /// The path as one sh word, with `$HOME` expanded by the shell.
    pub fn shell_word(&self) -> String {
        match self {
            Self::Absolute(path) => quote(path),
            Self::Home(rest) if rest.is_empty() => "\"$HOME\"".to_owned(),
            Self::Home(rest) => format!("\"$HOME\"/{}", quote(rest)),
        }
    }
}

/// The most folders one tree reads, the first included.
const MAX_LISTED: u32 = 400;

/// Reads `folder` and, `depth` levels down, the folders in it: each prints `path`, then
/// `git <origin>`, `full`, and `dir <name>` or `repo <name>` (a folder that is a checkout) lines
/// as they apply. Below the first, levels are read breadth first, at most [`MAX_LISTED`] folders
/// in all, skipping links, checkouts' insides and folders that cannot be opened.
pub fn script(folder: &FolderPath, depth: u8) -> String {
    format!(
        "one() {{\n\
         \x20 printf '\\n{MARK} path %s\\n' \"$1\"\n\
         \x20 if [ -e \"$1/.git\" ]; then printf '{MARK} git %s\\n' \"$(git -C \"$1\" remote get-url origin 2>/dev/null)\"; fi\n\
         \x20 for f in \"$1\"/* \"$1\"/.[!.]* \"$1\"/..?*; do\n\
         \x20   if [ -e \"$f\" ] || [ -L \"$f\" ]; then printf '{MARK} full\\n'; break; fi\n\
         \x20 done\n\
         \x20 n=0\n\
         \x20 for f in \"$1\"/*; do\n\
         \x20   [ -d \"$f\" ] || continue\n\
         \x20   f=${{f##*/}}\n\
         \x20   case $f in *'\n'*) continue;; esac\n\
         \x20   n=$((n+1))\n\
         \x20   if [ -e \"$1/$f/.git\" ]; then k=repo; else k=dir; fi\n\
         \x20   if [ $n -le {MAX_FOLDERS} ]; then printf '{MARK} %s %s\\n' $k \"$f\"; fi\n\
         \x20 done\n\
         \x20 if [ $n -gt {MAX_FOLDERS} ]; then printf '{MARK} more %s\\n' $((n-{MAX_FOLDERS})); fi\n\
         }}\n\
         opens() {{ [ -d \"$1\" ] && [ ! -L \"$1\" ] && [ -r \"$1\" ] && [ -x \"$1\" ]; }}\n\
         d={word}\n\
         if [ -d \"$d\" ]; then\n\
         \x20 cd \"$d\" 2>/dev/null || {{ printf '\\n{MARK} denied %s\\n' \"$d\"; exit 0; }}\n\
         \x20 top=$(pwd); top=${{top%/}}\n\
         \x20 one \"${{top:-/}}\"\n\
         \x20 listed=1\n\
         \x20 if [ {depth} -gt 0 ] && [ ! -e \"$top/.git\" ]; then\n\
         \x20   for c in \"$top\"/*; do\n\
         \x20     opens \"$c\" || continue\n\
         \x20     [ $listed -lt {MAX_LISTED} ] || break\n\
         \x20     listed=$((listed+1)); one \"$c\"\n\
         \x20   done\n\
         \x20 fi\n\
         \x20 if [ {depth} -gt 1 ] && [ ! -e \"$top/.git\" ]; then\n\
         \x20   for c in \"$top\"/*; do\n\
         \x20     opens \"$c\" && [ ! -e \"$c/.git\" ] || continue\n\
         \x20     for g in \"$c\"/*; do\n\
         \x20       opens \"$g\" || continue\n\
         \x20       [ $listed -lt {MAX_LISTED} ] || break 2\n\
         \x20       listed=$((listed+1)); one \"$g\"\n\
         \x20     done\n\
         \x20   done\n\
         \x20 fi\n\
         elif [ -e \"$d\" ]; then\n\
         \x20 printf '\\n{MARK} file %s\\n' \"$d\"\n\
         else\n\
         \x20 printf '\\n{MARK} missing %s\\n' \"$d\"\n\
         fi\n",
        word = folder.shell_word(),
    )
}

/// What the script found, read from its output.
#[derive(Debug, PartialEq)]
pub enum Seen {
    Folder(MachineFolder),
    /// A file is there, at this path.
    File(String),
    /// The folder is there but cannot be opened.
    Denied(String),
}

/// Reads the script's output: what it found at the folder asked for, then the folders it read
/// below it. Empty when it said nothing it means.
pub fn read(output: &str) -> Vec<Seen> {
    let mut seen = Vec::new();
    let mut block: Vec<(&str, &str)> = Vec::new();

    for line in output.lines() {
        let Some(said) = line.trim_end_matches('\r').strip_prefix(MARK) else {
            continue;
        };
        let said = said.strip_prefix(' ').unwrap_or(said);
        let (word, rest) = said.split_once(' ').unwrap_or((said, ""));

        if matches!(word, "path" | "missing" | "file" | "denied") && !block.is_empty() {
            seen.extend(read_one(&block));
            block.clear();
        }
        block.push((word, rest));
    }

    seen.extend(read_one(&block));
    seen
}

/// One folder's lines, as `read` splits them.
fn read_one(block: &[(&str, &str)]) -> Option<Seen> {
    let mut path = None;
    let mut exists = true;
    let mut origin = None;
    let mut full = false;
    let mut folders = Vec::new();
    let mut repos = Vec::new();
    let mut more = 0;

    for &(word, rest) in block {
        match word {
            "path" => path = Some(rest.to_owned()),
            "missing" => {
                path = Some(rest.to_owned());
                exists = false;
            }
            "file" => return Some(Seen::File(rest.to_owned())),
            "denied" => return Some(Seen::Denied(rest.to_owned())),
            "git" => origin = Some(rest.trim().to_owned()),
            "full" => full = true,
            "dir" => folders.push(rest.to_owned()),
            "repo" => {
                folders.push(rest.to_owned());
                repos.push(rest.to_owned());
            }
            "more" => more = rest.trim().parse().unwrap_or(0),
            _ => {}
        }
    }

    let path = path.filter(|path| path.starts_with('/'))?;
    let path = match path.trim_end_matches('/') {
        "" => "/".to_owned(),
        trimmed => trimmed.to_owned(),
    };
    folders.sort();
    repos.sort();
    Some(Seen::Folder(MachineFolder {
        parent: parent(&path),
        path,
        exists,
        folders,
        more,
        empty: !full,
        origin,
        repos,
    }))
}

/// The folder above `path`; `None` at the root.
fn parent(path: &str) -> Option<String> {
    if path == "/" {
        return None;
    }

    match path.rfind('/') {
        Some(0) => Some("/".to_owned()),
        Some(at) => Some(path[..at].to_owned()),
        None => None,
    }
}

/// Where an import into `seen` goes: the folder itself when a checkout, empty or missing, else `<folder>/<name>`.
pub fn destination(seen: &MachineFolder, name: &str) -> String {
    if !seen.exists || seen.empty || seen.origin.is_some() {
        seen.path.clone()
    } else if seen.path == "/" {
        format!("/{name}")
    } else {
        format!("{}/{name}", seen.path)
    }
}

/// The longest reading a folder may take.
const LOOK_LIMIT: Duration = Duration::from_secs(20);

/// What `folder` on the machine named `machine` holds.
pub async fn look(
    shell: &dyn Shell,
    machine: &str,
    folder: &FolderPath,
) -> Result<MachineFolder, ImportError> {
    let mut tree = tree(shell, machine, folder, 0).await?;
    Ok(tree.swap_remove(0))
}

/// What `folder` on the machine named `machine` holds, then the folders `depth` levels below
/// it, in one run.
pub async fn tree(
    shell: &dyn Shell,
    machine: &str,
    folder: &FolderPath,
    depth: u8,
) -> Result<Vec<MachineFolder>, ImportError> {
    let ran = shell
        .run(&script(folder, depth), LOOK_LIMIT)
        .await
        .map_err(|e| ImportError::Link {
            machine: machine.to_owned(),
            detail: format!("{e:#}"),
        })?;
    let mut seen = read(&ran.output).into_iter();
    let first = match seen.next() {
        Some(Seen::Folder(first)) => first,
        Some(Seen::File(path)) => {
            return Err(ImportError::NotAFolder {
                folder: path,
                machine: machine.to_owned(),
            });
        }
        Some(Seen::Denied(path)) => {
            return Err(ImportError::Unreadable {
                folder: path,
                machine: machine.to_owned(),
            });
        }
        None => {
            return Err(ImportError::NoListing {
                machine: machine.to_owned(),
            });
        }
    };

    Ok(std::iter::once(first)
        .chain(seen.filter_map(|seen| match seen {
            Seen::Folder(folder) => Some(folder),
            Seen::File(_) | Seen::Denied(_) => None,
        }))
        .collect())
}

/// Makes `folder` on the machine named `machine`, with the folders above it.
pub async fn make(
    shell: &dyn Shell,
    machine: &str,
    folder: &FolderPath,
) -> Result<(), ImportError> {
    let script = format!("mkdir -p -- {} 2>&1", folder.shell_word());
    let ran = shell
        .run(&script, LOOK_LIMIT)
        .await
        .map_err(|e| ImportError::Link {
            machine: machine.to_owned(),
            detail: format!("{e:#}"),
        })?;
    if ran.exit_code == Some(0) {
        return Ok(());
    }

    Err(ImportError::NoFolderMade {
        machine: machine.to_owned(),
        detail: ran.output.trim().to_owned(),
    })
}

#[cfg(test)]
#[path = "../tests/imports_folder.rs"]
mod tests;
