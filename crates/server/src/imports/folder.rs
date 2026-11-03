//! Folders the user picks on a machine, read through the node's shell so any readable folder works.

use super::{ImportError, clone::quote};
use crate::protocol::MachineFolder;

/// What the listing script prints before each line it means.
const MARK: &str = "BEEP_FOLDER:";
/// The most folders one listing names.
const MAX_FOLDERS: u32 = 500;
/// The longest path taken.
const MAX_PATH: usize = 1024;

/// A folder on a machine as given: absolute, or under `~`, which the machine's shell expands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FolderPath {
    /// `/` and the parts after it, without a trailing `/`.
    Absolute(String),
    /// The parts after `~/`; empty for the home folder itself.
    Home(String),
}

impl FolderPath {
    /// The folder `input` names, `None` when blank; refuses relative paths, `..` and control characters.
    pub(crate) fn parse(input: &str) -> Result<Option<Self>, ImportError> {
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
    pub(crate) fn shell_word(&self) -> String {
        match self {
            Self::Absolute(path) => quote(path),
            Self::Home(rest) if rest.is_empty() => "\"$HOME\"".to_owned(),
            Self::Home(rest) => format!("\"$HOME\"/{}", quote(rest)),
        }
    }
}

/// Reads one folder: prints `path`, then `git <origin>`, `full` and `dir <name>` lines as they apply.
pub(crate) fn script(folder: &FolderPath) -> String {
    format!(
        "d={word}\n\
         if [ -d \"$d\" ]; then\n\
         \x20 cd \"$d\" 2>/dev/null || {{ printf '\\n{MARK} denied %s\\n' \"$d\"; exit 0; }}\n\
         \x20 printf '\\n{MARK} path %s\\n' \"$(pwd)\"\n\
         \x20 if [ -e .git ]; then printf '{MARK} git %s\\n' \"$(git remote get-url origin 2>/dev/null)\"; fi\n\
         \x20 if [ -n \"$(ls -A 2>/dev/null)\" ]; then printf '{MARK} full\\n'; fi\n\
         \x20 n=0\n\
         \x20 for f in *; do\n\
         \x20   [ -d \"$f\" ] || continue\n\
         \x20   case $f in *'\n'*) continue;; esac\n\
         \x20   n=$((n+1))\n\
         \x20   if [ $n -le {MAX_FOLDERS} ]; then printf '{MARK} dir %s\\n' \"$f\"; fi\n\
         \x20 done\n\
         \x20 if [ $n -gt {MAX_FOLDERS} ]; then printf '{MARK} more %s\\n' $((n-{MAX_FOLDERS})); fi\n\
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
pub(crate) enum Seen {
    Folder(MachineFolder),
    /// A file is there, at this path.
    File(String),
    /// The folder is there but cannot be opened.
    Denied(String),
}

/// Reads the script's output; `None` when it said nothing it means.
pub(crate) fn read(output: &str) -> Option<Seen> {
    let mut path = None;
    let mut exists = true;
    let mut origin = None;
    let mut full = false;
    let mut folders = Vec::new();
    let mut more = 0;
    for line in output.lines() {
        let Some(said) = line.trim_end_matches('\r').strip_prefix(MARK) else {
            continue;
        };
        let said = said.strip_prefix(' ').unwrap_or(said);
        let (word, rest) = said.split_once(' ').unwrap_or((said, ""));
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
    Some(Seen::Folder(MachineFolder {
        parent: parent(&path),
        path,
        exists,
        folders,
        more,
        empty: !full,
        origin,
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
pub(crate) fn destination(seen: &MachineFolder, name: &str) -> String {
    if !seen.exists || seen.empty || seen.origin.is_some() {
        seen.path.clone()
    } else if seen.path == "/" {
        format!("/{name}")
    } else {
        format!("{}/{name}", seen.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{path::Path, process::Command};

    fn parse(input: &str) -> Result<Option<FolderPath>, ImportError> {
        FolderPath::parse(input)
    }

    #[test]
    fn takes_absolute_and_home_paths_only() {
        assert_eq!(parse("  "), Ok(None));
        assert_eq!(
            parse("/srv//code/./app/"),
            Ok(Some(FolderPath::Absolute("/srv/code/app".into())))
        );
        assert_eq!(parse("/"), Ok(Some(FolderPath::Absolute("/".into()))));
        assert_eq!(
            parse("~/Documents/my code"),
            Ok(Some(FolderPath::Home("Documents/my code".into())))
        );
        assert_eq!(parse("~"), Ok(Some(FolderPath::Home(String::new()))));
        for bad in [
            "code/app",
            "~user/code",
            "/srv/../etc",
            "/srv/a\nb",
            r"C:\code",
        ] {
            assert_eq!(parse(bad), Err(ImportError::BadFolder), "{bad:?}");
        }
        assert_eq!(
            parse(&format!("/{}", "a".repeat(MAX_PATH))),
            Err(ImportError::BadFolder)
        );
    }

    #[test]
    fn the_path_stays_one_shell_word() {
        let path = FolderPath::Absolute("/srv/it's $(rm -rf ~)".into());
        assert_eq!(path.shell_word(), r#"'/srv/it'\''s $(rm -rf ~)'"#);
        assert_eq!(FolderPath::Home(String::new()).shell_word(), "\"$HOME\"");
        assert_eq!(
            FolderPath::Home("a b".into()).shell_word(),
            "\"$HOME\"/'a b'"
        );
    }

    #[test]
    fn reads_what_the_script_printed() {
        let out = "motd\n\nBEEP_FOLDER: path /srv/code/\nBEEP_FOLDER: git git@github.com:o/r.git\n\
                   BEEP_FOLDER: full\nBEEP_FOLDER: dir src\nBEEP_FOLDER: dir  spaced \n\
                   BEEP_FOLDER: dir docs\nBEEP_FOLDER: more 3\n";
        let Some(Seen::Folder(seen)) = read(out) else {
            panic!("a folder");
        };
        assert_eq!(seen.path, "/srv/code");
        assert_eq!(seen.parent.as_deref(), Some("/srv"));
        assert!(seen.exists && !seen.empty);
        assert_eq!(seen.origin.as_deref(), Some("git@github.com:o/r.git"));
        assert_eq!(seen.folders, [" spaced ", "docs", "src"]);
        assert_eq!(seen.more, 3);

        let Some(Seen::Folder(missing)) = read("\nBEEP_FOLDER: missing /home/u/new\n") else {
            panic!("a missing folder");
        };
        assert!(!missing.exists && missing.empty);
        assert_eq!(
            read("BEEP_FOLDER: file /etc/hosts"),
            Some(Seen::File("/etc/hosts".into()))
        );
        assert_eq!(read("BEEP_FOLDER: path relative"), None);
        assert_eq!(read("nothing"), None);
        let Some(Seen::Folder(root)) = read("BEEP_FOLDER: path /") else {
            panic!("the root");
        };
        assert_eq!(root.parent, None);
    }

    fn folder(path: &str, exists: bool, empty: bool, origin: Option<&str>) -> MachineFolder {
        MachineFolder {
            path: path.into(),
            parent: None,
            exists,
            folders: Vec::new(),
            more: 0,
            empty,
            origin: origin.map(str::to_owned),
        }
    }

    #[test]
    fn other_files_put_the_clone_in_a_subfolder() {
        assert_eq!(destination(&folder("/w/x", false, true, None), "r"), "/w/x");
        assert_eq!(destination(&folder("/w/x", true, true, None), "r"), "/w/x");
        assert_eq!(
            destination(&folder("/w/x", true, false, Some("u")), "r"),
            "/w/x"
        );
        assert_eq!(
            destination(&folder("/w/x", true, false, None), "r"),
            "/w/x/r"
        );
        assert_eq!(destination(&folder("/", true, false, None), "r"), "/r");
    }

    fn run(script: &str, home: &Path) -> String {
        let out = Command::new("bash")
            .arg("-c")
            .arg("exec 2>&1; eval \"$BEEP_SCRIPT\" < /dev/null")
            .env("BEEP_SCRIPT", script)
            .env("HOME", home)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .expect("bash runs");
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    #[test]
    fn lists_a_real_folder() {
        let root = tempfile::tempdir().expect("a temp folder");
        let home = root.path();
        std::fs::create_dir_all(home.join("Documents/b dir")).expect("folders");
        std::fs::create_dir_all(home.join("Documents/a")).expect("folders");
        std::fs::create_dir_all(home.join("Documents/.hidden")).expect("folders");
        std::fs::write(home.join("Documents/notes.txt"), "x").expect("a file");
        let Some(Seen::Folder(seen)) =
            read(&run(&script(&FolderPath::Home("Documents".into())), home))
        else {
            panic!("a folder");
        };
        assert_eq!(seen.path, home.join("Documents").to_string_lossy());
        assert_eq!(seen.folders, ["a", "b dir"]);
        assert!(!seen.empty && seen.origin.is_none());

        let Some(Seen::Folder(empty)) =
            read(&run(&script(&FolderPath::Home("Documents/a".into())), home))
        else {
            panic!("a folder");
        };
        assert!(empty.empty && empty.folders.is_empty());

        let gone = script(&FolderPath::Home("nope".into()));
        let Some(Seen::Folder(gone)) = read(&run(&gone, home)) else {
            panic!("a missing folder");
        };
        assert!(!gone.exists);
        assert_eq!(gone.path, home.join("nope").to_string_lossy());

        let file = script(&FolderPath::Home("Documents/notes.txt".into()));
        assert!(matches!(read(&run(&file, home)), Some(Seen::File(_))));

        if Command::new("git").arg("--version").output().is_err() {
            eprintln!("skipped the checkout: no git");
            return;
        }
        let repo = home.join("Documents/b dir");
        for args in [
            &["init", "-q"][..],
            &["remote", "add", "origin", "https://example.com/o/r.git"][..],
        ] {
            let ok = Command::new("git")
                .args(args)
                .current_dir(&repo)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("HOME", home)
                .status()
                .expect("git runs")
                .success();
            assert!(ok, "git {args:?}");
        }
        let Some(Seen::Folder(checkout)) = read(&run(
            &script(&FolderPath::Home("Documents/b dir".into())),
            home,
        )) else {
            panic!("a checkout");
        };
        assert_eq!(
            checkout.origin.as_deref(),
            Some("https://example.com/o/r.git")
        );
    }
}
