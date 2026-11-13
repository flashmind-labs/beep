//! Where this machine keeps a session's transcript, and writing one in.

use super::{Harness, Stored};
use crate::agents::{
    Agents, claude,
    found::{self, Agent},
};
use anyhow::{Context, Result};
use beep_proto::watched;
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
            Harness::Flsh => Agent::Flsh,
        };
        let (stores, id) = (self.stores.clone(), id.to_owned());
        spawn_blocking(move || stores.file(Some(agent), &id))
            .await
            .ok()
            .flatten()
    }

    /// Whether flsh session `id` is here only in the database of a flsh
    /// from before 0.2.9.
    pub(super) async fn flsh_in_database(&self, id: &str) -> bool {
        let (stores, id) = (self.stores.clone(), id.to_owned());
        spawn_blocking(move || {
            stores
                .sessions(Some(Agent::Flsh))
                .iter()
                .any(|seen| seen.id == id && seen.file.is_none())
        })
        .await
        .unwrap_or(false)
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
    /// flsh's sessions folder.
    pub(crate) flsh: PathBuf,
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
        let flsh = found::flsh_sessions_dir()?;
        Some(Self {
            claude,
            codex,
            pi,
            flsh,
        })
    }

    fn of(&self, harness: Harness) -> &Path {
        match harness {
            Harness::Claude => &self.claude,
            Harness::Codex => &self.codex,
            Harness::Pi => &self.pi,
            Harness::Flsh => &self.flsh,
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

impl Agents {
    /// Refiles a closed conversation in another working folder.
    pub(crate) async fn relocate_session(
        &self,
        agent: &str,
        id: &str,
        destination: &str,
    ) -> anyhow::Result<()> {
        use anyhow::{Context, ensure};
        use beep_proto::node::agent::Status;
        use tokio::task::spawn_blocking;

        let _store = self.session_store.lock().await;
        let which: Agent = agent.parse().ok().context("unknown agent")?;
        ensure!(
            matches!(which, Agent::Claude | Agent::Codex | Agent::Pi),
            "this agent cannot move projects yet"
        );
        ensure!(
            !id.is_empty()
                && id.len() <= 128
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_')),
            "invalid session id"
        );
        let ours = self.get(id);
        let agent_id = match &ours {
            Some(session) => {
                ensure!(
                    session.agent == agent && session.info().state.status == Status::Ended,
                    "close the session before moving it"
                );
                watched::read(&session.live, |live| live.agent_session.clone())
                    .context("session has no agent id")?
            }
            None => id.to_owned(),
        };
        ensure!(
            self.lock()
                .values()
                .filter(|session| session.agent == agent
                    && watched::read(&session.live, |live| {
                        live.agent_session.as_deref() == Some(agent_id.as_str())
                    }))
                .all(|session| session.info().state.status == Status::Ended),
            "close the session before moving it"
        );
        self.check_closed_files(which, &agent_id, ours.is_some())
            .await?;
        let harness = Harness::try_from(which).map_err(|_| anyhow::anyhow!("unsupported agent"))?;
        let file = self
            .transcript_file(harness, &agent_id)
            .await
            .context("session transcript not found")?;
        let mut owned = self.owned();
        owned.remove(&agent_id);
        let old: PathBuf = match which {
            Agent::Claude => self
                .external
                .list(1, Some(agent_id.clone()), owned)
                .await?
                .pop()
                .context("session not found")?
                .cwd
                .into(),
            _ => {
                let stores = self.stores.clone();
                let find = agent_id.clone();
                spawn_blocking(move || stores.find(which, &find))
                    .await?
                    .context("session not found")?
                    .cwd
                    .into()
            }
        };
        ensure!(old.is_absolute(), "session working folder is unknown");
        let new = PathBuf::from(destination);
        ensure!(
            new.is_absolute() && new.is_dir(),
            "destination folder does not exist"
        );
        let agent_id_for_record = agent_id.clone();
        let moved_cwd = destination.to_owned();
        spawn_blocking(move || -> anyhow::Result<()> {
            let bytes = std::fs::read(&file)?;
            let name = file
                .file_name()
                .and_then(|name| name.to_str())
                .context("invalid transcript filename")?;
            let stored = super::rewrite(
                harness,
                &bytes,
                &agent_id,
                name,
                &super::folder::Folders {
                    old,
                    new,
                    workspace: None,
                },
            );
            let target = if harness == Harness::Codex {
                file.clone()
            } else {
                file.parent()
                    .and_then(Path::parent)
                    .context("transcript store unavailable")?
                    .join(&stored.path)
            };
            let side = file.with_extension("");
            let target_side = target.with_extension("");
            ensure!(
                target == file || !side.exists() || !target_side.exists(),
                "destination already contains session data"
            );
            ensure!(
                target == file || !target.exists(),
                "destination already contains this session"
            );
            write(&target, &stored)?;

            if target != file {
                let moving_side = harness == Harness::Claude && side.is_dir();
                if moving_side && let Err(error) = std::fs::rename(&side, &target_side) {
                    let _ = std::fs::remove_file(&target);
                    return Err(error.into());
                }

                if let Err(error) = std::fs::remove_file(&file) {
                    if moving_side {
                        let _ = std::fs::rename(&target_side, &side);
                    }
                    let _ = std::fs::remove_file(&target);
                    return Err(error.into());
                }
            }

            Ok(())
        })
        .await??;

        for (key, session) in self.lock().iter() {
            if session.agent == agent
                && (key == id
                    || watched::read(&session.live, |live| {
                        live.agent_session.as_deref() == Some(agent_id_for_record.as_str())
                    }))
            {
                session.moved_to(&moved_cwd);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
#[path = "../../tests/agents_moving_store.rs"]
mod tests;
