//! An app opened from Finder or at login gets launchd's bare `PATH`, so the
//! node's shell would not find Homebrew or cargo tools. This takes the
//! user's login shell `PATH` instead, as `beep-node service install` keeps
//! the installing shell's on Linux.
//!
//! A login shell takes from 0.2 to 3 seconds, so the `PATH` it gave last
//! run is kept in the app data folder and taken at once; the shell is asked
//! again in the background, and a changed `PATH` applies from the next start.
//! Only the first start waits for the shell.

use std::{
    env, fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

/// How long a login shell may take to print its `PATH`.
const DEADLINE: Duration = Duration::from_secs(3);
/// Brackets the `PATH` in the shell's output, past anything rc files print.
const MARK: &str = "__beep_path__";

/// The file in the app data folder that keeps the login shell's `PATH`.
const SAVED: &str = "login-path";

/// Sets `PATH` to the login shell's when the app did not inherit one: the one
/// kept from the last run, or the shell's own on the first. `identifier` names
/// the app data folder. Returns the background read that keeps it current.
///
/// # Safety
///
/// Changes the environment, so it must run before any other thread starts.
#[must_use]
pub unsafe fn adopt(identifier: &str) -> Option<Refresh> {
    let current = env::var("PATH").unwrap_or_default();

    if !bare(&current) {
        return None;
    }

    let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let file = dirs::data_dir().map(|dir| dir.join(identifier).join(SAVED));

    if let Some(file) = &file
        && let Some(saved) = read_saved(file)
    {
        // SAFETY: the caller runs this before any other thread exists.
        unsafe { env::set_var("PATH", &saved) };
        return Some(Refresh {
            shell,
            file: file.clone(),
            applied: saved,
        });
    }

    match login_path(&shell) {
        Some(path) => {
            if let Some(file) = &file {
                save(file, &path);
            }

            // SAFETY: the caller runs this before any other thread exists.
            unsafe { env::set_var("PATH", path) };
        }
        None => log::warn!("could not read the login shell's PATH; keeping {current}"),
    }

    None
}

/// Asks the login shell for its `PATH` again, after the app took the kept one.
pub struct Refresh {
    shell: String,
    file: PathBuf,
    applied: String,
}

impl Refresh {
    /// Keeps the shell's `PATH` for the next start when it changed. This run
    /// keeps the one it took: the environment cannot change once threads run.
    /// It blocks for as long as the shell takes, so it runs on a thread of its own.
    pub fn run(self) {
        match login_path(&self.shell) {
            Some(path) if path != self.applied => {
                log::info!("the login shell's PATH changed; it applies from the next start");
                save(&self.file, &path);
            }
            Some(_) => {}
            None => log::warn!("could not read the login shell's PATH again"),
        }
    }
}

fn read_saved(file: &Path) -> Option<String> {
    let saved = fs::read_to_string(file).ok()?;
    (!saved.trim().is_empty()).then_some(saved)
}

/// Writes `path` to `file` through a partial file, so a reader never sees half of it.
fn save(file: &Path, path: &str) {
    let mut partial = file.as_os_str().to_owned();
    partial.push(".part");
    let written = file
        .parent()
        .map_or(Ok(()), fs::create_dir_all)
        .and_then(|()| fs::write(&partial, path))
        .and_then(|()| fs::rename(&partial, file));

    if let Err(e) = written {
        log::warn!("keeping the login shell's PATH in {}: {e}", file.display());
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
#[path = "../tests/platform_login_path.rs"]
mod tests;
