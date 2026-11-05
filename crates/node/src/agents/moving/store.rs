//! Where this machine keeps a session's transcript, and writing one in.

use super::{Harness, Stored};
use crate::agents::{Agents, claude, found::Agent};
use anyhow::{Context, Result};
use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
};
use tokio::task::spawn_blocking;

impl Agents {
    /// The transcript file of session `id` of `harness` here, when there
    /// is one.
    pub(super) async fn transcript_file(&self, harness: Harness, id: &str) -> Option<PathBuf> {
        let agent = match harness {
            Harness::Claude => return self.external.file(id).await.ok().flatten(),
            Harness::Codex => Agent::Codex,
            Harness::Pi => Agent::Pi,
        };
        let (stores, id) = (self.stores.clone(), id.to_owned());
        spawn_blocking(move || stores.file(Some(agent), &id))
            .await
            .ok()
            .flatten()
    }
}

/// The folders the harnesses keep their sessions in on this machine, which
/// may not exist yet.
#[derive(Debug, Clone)]
pub(crate) struct StoreRoots {
    /// Claude Code's `projects` folder.
    pub(crate) claude: PathBuf,
    /// Codex's home; its sessions are in `sessions/`.
    pub(crate) codex: PathBuf,
    /// pi's sessions folder.
    pub(crate) pi: PathBuf,
}

impl StoreRoots {
    /// As the harnesses find them: their environment variables, else the
    /// home folder. `None` without a home folder to fall back on.
    pub(crate) fn locate() -> Option<Self> {
        let home = dirs::home_dir();
        let var = |name: &str| env::var_os(name).filter(|value| !value.is_empty());
        let claude = claude::config_dir()?.join("projects");
        let codex = match var("CODEX_HOME") {
            Some(dir) => PathBuf::from(dir),
            None => home.as_ref()?.join(".codex"),
        };
        let pi = match (
            var("PI_CODING_AGENT_SESSION_DIR"),
            var("PI_CODING_AGENT_DIR"),
        ) {
            (Some(dir), _) => PathBuf::from(dir),
            (None, Some(dir)) => PathBuf::from(dir).join("sessions"),
            (None, None) => home?.join(".pi/agent/sessions"),
        };
        Some(Self { claude, codex, pi })
    }

    fn of(&self, harness: Harness) -> &Path {
        match harness {
            Harness::Claude => &self.claude,
            Harness::Codex => &self.codex,
            Harness::Pi => &self.pi,
        }
    }

    /// Where `stored` goes, as a full path.
    pub(crate) fn path(&self, harness: Harness, stored: &Stored) -> PathBuf {
        self.of(harness).join(&stored.path)
    }
}

/// Writes `stored` at `path`, readable by this user only. A transcript
/// already there that is not a start of it is kept beside it as
/// `<name>.replaced-<time>` (each replacement under a name of its own, so a
/// second never destroys the first); one that is, is replaced, as the new
/// one only adds to it.
pub(super) fn write(path: &Path, stored: &Stored) -> Result<()> {
    let dir = path.parent().context("the transcript has no folder")?;
    fs::create_dir_all(dir)?;
    if let Ok(before) = fs::read(path)
        && !stored.bytes.starts_with(&before)
    {
        fs::rename(path, aside_name(path))?;
    }
    let mut partial = path.as_os_str().to_owned();
    partial.push(".partial");
    let partial = PathBuf::from(partial);
    let mut file = private(&partial)?;
    file.write_all(&stored.bytes)?;
    file.sync_all()?;
    fs::rename(partial, path)?;
    Ok(())
}

/// A name beside `path` for the transcript it replaces, not taken yet.
fn aside_name(path: &Path) -> PathBuf {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis());
    let mut n = 0;
    loop {
        let mut aside = path.as_os_str().to_owned();
        aside.push(format!(".replaced-{now}"));
        if n > 0 {
            aside.push(format!("-{n}"));
        }
        let aside = PathBuf::from(aside);
        if !aside.exists() {
            return aside;
        }
        n += 1;
    }
}

fn private(path: &Path) -> std::io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn stored(bytes: &[u8]) -> Stored {
        Stored {
            bytes: bytes.to_vec(),
            path: PathBuf::from("s.jsonl"),
        }
    }

    #[test]
    fn every_replacement_is_kept_under_a_name_of_its_own() {
        let dir = TempDir::new().expect("a temp folder");
        let path = dir.path().join("s.jsonl");
        fs::write(&path, b"first\n").expect("a transcript");
        write(&path, &stored(b"second\n")).expect("the first replacement");
        write(&path, &stored(b"third\n")).expect("the second replacement");
        assert_eq!(fs::read(&path).unwrap(), b"third\n");
        let mut aside: Vec<Vec<u8>> = fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .filter(|f| f.file_name().to_string_lossy().contains(".replaced-"))
            .map(|f| fs::read(f.path()).unwrap())
            .collect();
        aside.sort();
        assert_eq!(aside, [b"first\n".to_vec(), b"second\n".to_vec()]);
    }

    #[test]
    fn a_transcript_the_new_one_extends_is_replaced_without_a_copy() {
        let dir = TempDir::new().expect("a temp folder");
        let path = dir.path().join("s.jsonl");
        fs::write(&path, b"first\n").expect("a transcript");
        write(&path, &stored(b"first\nmore\n")).expect("an extension");
        assert_eq!(fs::read(&path).unwrap(), b"first\nmore\n");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
