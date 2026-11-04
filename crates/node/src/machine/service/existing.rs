//! Which folder an installed service runs from, so `mesh join` never
//! replaces a service that serves another node. The parsers read the text
//! each manager keeps: a systemd unit, a launchd plist, a Windows task.

use anyhow::{Context, Result, bail};
use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use super::{Manager, launchd, systemd, windows};

/// The service of this machine's manager, as far as its definition says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Installed {
    /// No service is installed.
    No,
    /// One is installed and runs from this folder.
    From(PathBuf),
    /// One is installed but its definition does not name a folder.
    Unreadable,
}

/// What the installed service is, read from its definition.
pub(super) fn installed(manager: Manager) -> Result<Installed> {
    let (path, parse): (PathBuf, fn(&str) -> Option<PathBuf>) = match manager {
        Manager::Systemd => (systemd::unit_path()?, systemd::dir_in),
        Manager::Launchd => (launchd::plist_path()?, launchd::dir_in),
        Manager::TaskScheduler => {
            if !windows::installed() {
                return Ok(Installed::No);
            }
            let path = windows::task_path()?;
            return match fs::read(&path) {
                Ok(bytes) => Ok(found(windows::dir_in(&windows::from_utf16(&bytes)))),
                // Installed by hand, or the file was removed.
                Err(e) if e.kind() == ErrorKind::NotFound => Ok(Installed::Unreadable),
                Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
            };
        }
    };
    match fs::read_to_string(&path) {
        Ok(text) => Ok(found(parse(&text))),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(Installed::No),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

fn found(dir: Option<PathBuf>) -> Installed {
    dir.map_or(Installed::Unreadable, Installed::From)
}

/// What `join` into a folder does about the service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// None is installed: write it and start it.
    Install,
    /// The installed one runs from this folder: start it as it is, so its
    /// own arguments (such as `--workspace`) stay.
    Start,
}

/// What `join` into `dir` does about the service, or why it must not touch
/// the one installed.
pub(super) fn decide(installed: &Installed, dir: &Path) -> Result<Action> {
    let here = dir.display();
    match installed {
        Installed::No => Ok(Action::Install),
        Installed::From(other) if same_folder(other, dir) => Ok(Action::Start),
        Installed::From(other) => {
            let other = other.display();
            bail!(
                "A beep-node service already runs from {other}. Join with: beep-node --dir \
                 {other} mesh join <ticket>, or to run a second node, `beep-node --dir {here} \
                 run` in another terminal, then join again"
            )
        }
        Installed::Unreadable => bail!(
            "A beep-node service is already installed and its folder cannot be read. Join \
             with its own folder: beep-node --dir <folder> mesh join <ticket>, or to run a \
             second node, `beep-node --dir {here} run` in another terminal, then join again"
        ),
    }
}

/// Whether two paths name one folder, resolving links when they exist and
/// ignoring a Windows `\\?\` prefix.
fn same_folder(a: &Path, b: &Path) -> bool {
    let plain = |path: &Path| {
        let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
        let text = path.display().to_string();
        let text = text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned();
        if cfg!(windows) {
            text.to_ascii_lowercase()
        } else {
            text
        }
    };
    plain(a) == plain(b)
}

/// The argument after `--dir` in a node's command line.
pub(super) fn dir_arg(args: &[String]) -> Option<PathBuf> {
    let at = args.iter().position(|arg| arg == "--dir")?;
    args.get(at + 1).map(PathBuf::from)
}

/// Text with the five XML entities replaced.
pub(super) fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn from(dir: &str) -> Installed {
        Installed::From(PathBuf::from(dir))
    }

    #[test]
    fn no_service_lets_a_join_install_one() {
        let action = decide(&Installed::No, Path::new("/srv/new")).unwrap();
        assert_eq!(action, Action::Install);
    }

    #[test]
    fn a_service_from_this_folder_is_started_not_rewritten() {
        let action = decide(&from("/srv/node"), Path::new("/srv/node")).unwrap();
        assert_eq!(action, Action::Start);
    }

    #[test]
    fn a_service_from_another_folder_is_left_alone() {
        let e = decide(&from("/srv/node"), Path::new("/srv/other")).unwrap_err();
        let e = e.to_string();
        assert!(
            e.starts_with(
                "A beep-node service already runs from /srv/node. Join with: beep-node --dir \
                 /srv/node mesh join <ticket>"
            ),
            "{e}"
        );
        assert!(
            e.contains("`beep-node --dir /srv/other run` in another terminal"),
            "{e}"
        );
    }

    #[test]
    fn a_service_whose_folder_is_unknown_is_left_alone() {
        let e = decide(&Installed::Unreadable, Path::new("/srv/other")).unwrap_err();
        assert!(e.to_string().contains("already installed"), "{e}");
    }

    #[test]
    fn dir_arg_reads_the_folder_after_the_flag() {
        let args: Vec<String> = ["/bin/beep-node", "--dir", "/a b", "run"]
            .map(String::from)
            .to_vec();
        assert_eq!(dir_arg(&args), Some(PathBuf::from("/a b")));
        assert_eq!(dir_arg(&args[..2]), None);
    }
}
