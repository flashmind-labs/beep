//! An app opened from Finder or at login gets launchd's bare `PATH`, so the
//! node's shell would not find Homebrew or cargo tools. This takes the
//! user's login shell `PATH` instead, as `beep-node service install` keeps
//! the installing shell's on Linux.

use std::{
    env,
    io::Read,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

/// How long a login shell may take to print its `PATH`.
const DEADLINE: Duration = Duration::from_secs(3);
/// Brackets the `PATH` in the shell's output, past anything rc files print.
const MARK: &str = "__beep_path__";

/// Sets `PATH` to the login shell's when the app did not inherit one.
///
/// # Safety
///
/// Changes the environment, so it must run before any other thread starts.
pub unsafe fn adopt() {
    let current = env::var("PATH").unwrap_or_default();
    if !bare(&current) {
        return;
    }
    let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    match login_path(&shell) {
        // SAFETY: the caller runs this before any other thread exists.
        Some(path) => unsafe { env::set_var("PATH", path) },
        None => log::warn!("could not read the login shell's PATH; keeping {current}"),
    }
}

/// Whether `path` holds only system directories, as launchd gives apps.
fn bare(path: &str) -> bool {
    env::split_paths(path)
        .filter(|dir| !dir.as_os_str().is_empty())
        .all(|dir| {
            ["/usr/bin", "/bin", "/usr/sbin", "/sbin"]
                .iter()
                .any(|system| dir.as_os_str() == *system)
        })
}

/// `PATH` as an interactive login `shell` sets it, or `None` when it fails
/// or takes too long.
fn login_path(shell: &str) -> Option<String> {
    let mut child = Command::new(shell)
        .args([
            "-l",
            "-i",
            "-c",
            &format!("printf '{MARK}%s{MARK}' \"$PATH\""),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() < DEADLINE => thread::sleep(Duration::from_millis(20)),
            _ => {
                // A shell stuck on a prompt; nothing to keep from it.
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let mut out = String::new();
    child.stdout.take()?.read_to_string(&mut out).ok()?;
    parse(&out)
}

/// The text between the marks, when it is a usable `PATH`.
fn parse(out: &str) -> Option<String> {
    let (_, rest) = out.split_once(MARK)?;
    let (path, _) = rest.split_once(MARK)?;
    (!path.trim().is_empty()).then(|| path.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_launchd_path_counts_as_bare() {
        assert!(bare("/usr/bin:/bin:/usr/sbin:/sbin"));
        assert!(bare(""));
        assert!(!bare("/opt/homebrew/bin:/usr/bin:/bin"));
    }

    #[test]
    fn the_path_is_read_past_what_rc_files_print() {
        let out = format!("welcome back\n{MARK}/opt/homebrew/bin:/usr/bin{MARK}");
        assert_eq!(parse(&out).as_deref(), Some("/opt/homebrew/bin:/usr/bin"));
        assert_eq!(parse("no marks here"), None);
        assert_eq!(parse(&format!("{MARK}{MARK}")), None);
    }

    #[test]
    fn a_login_shell_gives_its_path() {
        let path = login_path("/bin/sh").expect("sh prints its PATH");
        assert!(!path.is_empty());
    }
}
