//! Which folders the server may reach on this node through the file
//! requests (`SessionsRequest::File`, `Folder`, `ReadFile`, `WriteFile`).
//! Read and written: the node's workspace and shared folders, and the
//! folders coding agent sessions work in. Only read: the harnesses' session
//! logs and Claude Code's temp folder, where it keeps the files it makes.
//! The custom agents list (`SessionsRequest::CustomAgents`) reads only in
//! the work folders.
//! Every path is checked with its links and `..` resolved.

use super::{Agents, media_file::resolve};
use crate::{claude, found::LogRoots};
use anyhow::{Context, Result, bail};
use std::{
    fs,
    io::ErrorKind,
    path::{Component, Path, PathBuf},
};
use tokio::task::spawn_blocking;

/// What a request does with what it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Access {
    Read,
    Write,
    /// Read, but only in a work folder: a project folder asked for.
    Work,
}

/// The folders a request may reach, as they were named.
#[derive(Debug, Default, Clone)]
pub(super) struct Reach {
    /// Read and written.
    pub(super) work: Vec<PathBuf>,
    /// Only read.
    pub(super) read: Vec<PathBuf>,
    /// Never written, even inside a work folder.
    pub(super) no_write: Vec<PathBuf>,
}

impl Reach {
    /// Whether `real`, already resolved by [`real`], lies in a folder
    /// `access` may reach. Blocks on the file system.
    pub(super) fn holds(&self, real: &Path, access: Access) -> bool {
        let inside = |folders: &[PathBuf]| {
            folders
                .iter()
                .filter_map(|folder| fs::canonicalize(folder).ok())
                // A session in `/` would open the whole disk.
                .filter(|folder| folder.parent().is_some())
                .any(|folder| real.starts_with(folder))
        };
        match access {
            Access::Read => inside(&self.work) || inside(&self.read),
            Access::Write => inside(&self.work) && !inside(&self.no_write),
            Access::Work => inside(&self.work),
        }
    }
}

/// The refusal for `asked`, outside what `access` may reach.
pub(super) fn refused(asked: &str, access: Access) -> anyhow::Error {
    let what = match access {
        Access::Read => "read",
        Access::Write => "write",
        Access::Work => "list custom agents in",
    };
    anyhow::anyhow!(
        "{asked} is outside the folders Beep may {what} on this node: its workspace, \
         shared folders and the folders of agent sessions"
    )
}

/// `path`, absolute, with its links and `..` resolved. A path not there
/// yet is its nearest folder that is, resolved, with the rest joined.
/// Blocks on the file system.
pub(super) fn real(path: &Path) -> Result<PathBuf> {
    let mut missing = Vec::new();
    let mut at = path;
    loop {
        match fs::canonicalize(at) {
            Ok(mut found) => {
                for name in missing.iter().rev() {
                    found.push(name);
                }
                return Ok(found);
            }
            Err(e) if e.kind() == ErrorKind::NotFound => {
                let mut parts = at.components();
                match parts.next_back() {
                    Some(Component::Normal(name)) => missing.push(name.to_owned()),
                    _ => bail!("{} leads nowhere it can be checked", path.display()),
                }
                at = parts.as_path();
            }
            Err(e) => return Err(e).with_context(|| format!("could not open {}", path.display())),
        }
    }
}

impl Agents {
    /// Lets file requests reach `folders`, the workspace and the folders
    /// shared in `node.toml`, beside the sessions' folders. Set once.
    pub fn share_folders(&self, folders: Vec<PathBuf>) {
        if self.folders.set(folders).is_err() {
            log::warn!("the shared folders were set already");
        }
    }

    /// What file requests may reach, the sessions' folders among them;
    /// those of sessions started outside Beep only with `sighted`.
    fn reach(&self, sighted: Vec<PathBuf>) -> Reach {
        let mut work: Vec<PathBuf> = self.folders.get().cloned().unwrap_or_default();
        work.extend(
            self.lock()
                .values()
                .map(|session| PathBuf::from(session.info().cwd)),
        );
        work.extend(sighted);
        let logs = LogRoots::locate();
        let read = [
            claude::projects_dir(),
            logs.codex.as_ref().map(|codex| codex.join("sessions")),
            logs.codex.map(|codex| codex.join("archived_sessions")),
            logs.pi,
            logs.flsh,
            // OpenCode's database, or its folder of them.
            logs.opencode,
            claude_temp(),
        ]
        .into_iter()
        .flatten()
        .collect();
        Reach {
            work,
            read,
            no_write: claude_temp().into_iter().collect(),
        }
    }

    /// `asked`, absolute or starting `~/`, with its links and `..`
    /// resolved, once it is inside the folders `access` may reach; refused
    /// otherwise.
    pub(super) async fn reachable(&self, asked: &str, access: Access) -> Result<PathBuf> {
        let home = dirs::home_dir();
        let reach = self.reach(Vec::new());
        let owned = asked.to_owned();
        let (real, held) = spawn_blocking(move || -> Result<(PathBuf, bool)> {
            let real = real(&resolve(&owned, home.as_deref())?)?;
            let held = reach.holds(&real, access);
            Ok((real, held))
        })
        .await
        .context("the path check stopped")??;
        if held {
            return Ok(real);
        }
        // The folders of sessions started outside Beep cost a look in the
        // harnesses' logs, so only when the others did not hold it.
        let sighted = self
            .sightings()
            .await?
            .into_iter()
            .map(|seen| seen.cwd)
            .collect();
        let reach = self.reach(sighted);
        let held = spawn_blocking(move || reach.holds(&real, access).then_some(real))
            .await
            .context("the path check stopped")?;
        held.ok_or_else(|| refused(asked, access))
    }
}

/// Claude Code's temp folder for this user. See [`claude_temp_in`].
pub(super) fn claude_temp() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        let uid = rustix::process::getuid().as_raw();
        Some(claude_temp_in(
            std::env::var_os(CLAUDE_TEMP_VAR).as_deref(),
            uid,
        ))
    }
    #[cfg(not(unix))]
    {
        None
    }
}

/// What moves Claude Code's temp folder.
#[cfg(unix)]
const CLAUDE_TEMP_VAR: &str = "CLAUDE_CODE_TMPDIR";

/// Claude Code's temp folder for `uid`: `claude-<uid>` in `set`, the
/// value of `CLAUDE_CODE_TMPDIR`, or else in `/tmp`. Claude Code uses
/// `/tmp` on macOS too, never `TMPDIR` or the per-user
/// `/var/folders/.../T/`, so the folder does not depend on the
/// environment launchd gives the node's service. A `CLAUDE_CODE_TMPDIR`
/// set only in the user's shell is not seen by the service.
#[cfg(unix)]
fn claude_temp_in(set: Option<&std::ffi::OsStr>, uid: u32) -> PathBuf {
    let base = set
        .filter(|base| !base.is_empty())
        .map_or_else(|| PathBuf::from("/tmp"), PathBuf::from);
    base.join(format!("claude-{uid}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn a_missing_path_resolves_through_its_folder() {
        let dir = TempDir::new().unwrap();
        let base = fs::canonicalize(dir.path()).unwrap();
        let asked = dir.path().join("new/deeper/file.mp4");
        assert_eq!(real(&asked).unwrap(), base.join("new/deeper/file.mp4"));
        let up = dir.path().join("a/../../x.png");
        assert!(real(&up).is_err());
    }

    #[test]
    fn reads_reach_further_than_writes() {
        let work = TempDir::new().unwrap();
        let logs = TempDir::new().unwrap();
        let reach = Reach {
            work: vec![work.path().to_owned()],
            read: vec![logs.path().to_owned()],
            no_write: Vec::new(),
        };
        let in_logs = real(&logs.path().join("a.jsonl")).unwrap();
        assert!(reach.holds(&in_logs, Access::Read));
        assert!(!reach.holds(&in_logs, Access::Write));
        let in_work = real(&work.path().join("src")).unwrap();
        assert!(reach.holds(&in_work, Access::Write));
        assert!(!reach.holds(Path::new("/etc"), Access::Read));
    }

    /// `asked` checked as `reachable` does, without the sessions found in
    /// the harnesses' logs.
    fn check(reach: &Reach, asked: &Path, access: Access) -> Result<PathBuf> {
        let asked = asked.to_str().expect("temp paths are UTF-8");
        let found = real(&resolve(asked, None)?)?;
        if reach.holds(&found, access) {
            Ok(found)
        } else {
            Err(refused(asked, access))
        }
    }

    fn project_reach(project: &TempDir) -> Reach {
        Reach {
            work: vec![project.path().to_owned()],
            ..Reach::default()
        }
    }

    #[test]
    fn a_project_file_is_reached() {
        let project = TempDir::new().unwrap();
        fs::create_dir(project.path().join("src")).unwrap();
        fs::write(project.path().join("src/clip.mp4"), "video").unwrap();
        let reach = project_reach(&project);
        let file = check(&reach, &project.path().join("src/clip.mp4"), Access::Read).unwrap();
        assert_eq!(fs::read(file).unwrap(), b"video");
        let folder = check(&reach, &project.path().join("src"), Access::Write).unwrap();
        assert!(folder.is_dir());
    }

    #[test]
    fn a_path_outside_is_refused() {
        let project = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        fs::write(outside.path().join("secret.png"), "secret").unwrap();
        let reach = project_reach(&project);
        for access in [Access::Read, Access::Write] {
            let e = check(&reach, &outside.path().join("secret.png"), access).unwrap_err();
            assert!(e.to_string().contains("outside the folders"), "{e:#}");
        }
        let e = check(&reach, Path::new("/etc/passwd"), Access::Read).unwrap_err();
        assert!(e.to_string().contains("outside the folders"), "{e:#}");
    }

    #[test]
    fn a_dot_dot_escape_is_refused() {
        let project = TempDir::new().unwrap();
        fs::create_dir(project.path().join("src")).unwrap();
        let reach = project_reach(&project);
        let escape = project.path().join("src/../../etc");
        assert!(check(&reach, &escape, Access::Read).is_err());
        let missing = project.path().join("gone/../../../etc/passwd");
        assert!(check(&reach, &missing, Access::Read).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_escape_is_refused() {
        let project = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        fs::write(outside.path().join("id_key"), "secret").unwrap();
        std::os::unix::fs::symlink(outside.path(), project.path().join("out")).unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("id_key"),
            project.path().join("key.png"),
        )
        .unwrap();
        let reach = project_reach(&project);
        for asked in ["out", "out/id_key", "key.png", "out/not-yet.png"] {
            let e = check(&reach, &project.path().join(asked), Access::Read).unwrap_err();
            assert!(
                e.to_string().contains("outside the folders"),
                "{asked}: {e:#}"
            );
        }
    }

    /// A reach with `temp` as Claude Code's temp folder, which a session
    /// also works in.
    fn temp_reach(temp: &TempDir) -> Reach {
        Reach {
            work: vec![temp.path().to_owned()],
            read: vec![temp.path().to_owned()],
            no_write: vec![temp.path().to_owned()],
        }
    }

    #[test]
    fn claude_codes_temp_folder_is_read() {
        let temp = TempDir::new().unwrap();
        fs::create_dir(temp.path().join("scratchpad")).unwrap();
        fs::write(temp.path().join("scratchpad/plot.png"), "png").unwrap();
        let reach = Reach {
            read: vec![temp.path().to_owned()],
            ..Reach::default()
        };
        let file = check(
            &reach,
            &temp.path().join("scratchpad/plot.png"),
            Access::Read,
        )
        .unwrap();
        assert_eq!(fs::read(file).unwrap(), b"png");
        assert!(check(&reach, &temp.path().join("scratchpad"), Access::Read).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_out_of_claude_codes_temp_folder_is_refused() {
        let temp = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        fs::write(outside.path().join("notes.txt"), "secret").unwrap();
        std::os::unix::fs::symlink(outside.path(), temp.path().join("link")).unwrap();
        let reach = temp_reach(&temp);
        let e = check(&reach, &temp.path().join("link/notes.txt"), Access::Read).unwrap_err();
        assert!(e.to_string().contains("outside the folders"), "{e:#}");
    }

    #[test]
    fn claude_codes_temp_folder_is_never_written() {
        let temp = TempDir::new().unwrap();
        fs::create_dir(temp.path().join("scratchpad")).unwrap();
        let reach = temp_reach(&temp);
        let e = check(&reach, &temp.path().join("scratchpad"), Access::Write).unwrap_err();
        assert!(e.to_string().contains("may write"), "{e:#}");
        // Read only: the same folder reads.
        assert!(check(&reach, &temp.path().join("scratchpad"), Access::Read).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn claude_codes_temp_folder_is_in_tmp_by_uid() {
        use std::ffi::OsStr;
        // Wherever TMPDIR points, as it does under launchd on macOS.
        assert_eq!(claude_temp_in(None, 501), Path::new("/tmp/claude-501"));
        assert_eq!(
            claude_temp_in(Some(OsStr::new("")), 1000),
            Path::new("/tmp/claude-1000")
        );
        assert_eq!(
            claude_temp_in(Some(OsStr::new("/srv/scratch")), 501),
            Path::new("/srv/scratch/claude-501")
        );
        let uid = rustix::process::getuid().as_raw();
        let set = std::env::var_os(CLAUDE_TEMP_VAR);
        assert_eq!(claude_temp(), Some(claude_temp_in(set.as_deref(), uid)));
    }

    #[tokio::test]
    async fn a_shared_folder_is_reached_through_the_agents() {
        let project = TempDir::new().unwrap();
        fs::write(project.path().join("model.glb"), "glTF").unwrap();
        let agents = Agents::new(Vec::new());
        agents.share_folders(vec![project.path().to_owned()]);
        let asked = project.path().join("model.glb");
        let real = agents
            .reachable(asked.to_str().unwrap(), Access::Write)
            .await
            .unwrap();
        assert_eq!(real, fs::canonicalize(&asked).unwrap());
    }

    #[test]
    fn custom_agents_are_listed_only_in_a_work_folder() {
        let project = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let logs = TempDir::new().unwrap();
        fs::create_dir_all(project.path().join(".claude/agents")).unwrap();
        fs::write(
            project.path().join(".claude/agents/reviewer.md"),
            "---\nname: reviewer\n---\nReview.",
        )
        .unwrap();
        fs::create_dir_all(outside.path().join(".claude/agents")).unwrap();
        let reach = Reach {
            work: vec![project.path().to_owned()],
            read: vec![logs.path().to_owned()],
            no_write: Vec::new(),
        };
        let real = check(&reach, project.path(), Access::Work).unwrap();
        let agents = super::super::custom_agents::list(&real, None).unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].name, "reviewer");
        for refused_folder in [outside.path(), logs.path(), Path::new("/etc")] {
            let e = check(&reach, refused_folder, Access::Work).unwrap_err();
            assert!(e.to_string().contains("outside the folders"), "{e:#}");
        }
        assert!(check(&reach, Path::new("relative/path"), Access::Work).is_err());
    }

    #[test]
    fn the_root_folder_is_never_a_reach() {
        let reach = Reach {
            work: vec![PathBuf::from("/")],
            ..Reach::default()
        };
        assert!(!reach.holds(Path::new("/etc/passwd"), Access::Read));
    }
}
