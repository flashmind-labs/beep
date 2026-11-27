//! Where this machine keeps a session's transcript, and writing one in.

use super::{Harness, Stored};
use crate::agents::{delete::Delete, listings::Listings, sessions};
use crate::logs::claude_dir;
use crate::logs::found::{self, Agent};
use anyhow::{Context, Result, ensure};
use beep_proto::SessionId;
use beep_proto::node::agent::Status;
use beep_proto::watched;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use tokio::task::spawn_blocking;

impl Listings {
    /// The transcript file of session `id` of `harness` here, when there
    /// is one.
    pub(super) async fn transcript_file(
        &self,
        harness: Harness,
        id: &SessionId,
    ) -> Option<PathBuf> {
        self.store_file(Some(Into::<Agent>::into(harness)), id)
            .await
            .ok()
            .flatten()
    }

    /// Whether flsh session `id` is here only in the database of a flsh
    /// from before 0.2.9.
    pub(super) async fn flsh_in_database(&self, id: &SessionId) -> bool {
        let (stores, id) = (self.stores().clone(), id.to_owned());
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
        let claude = claude_dir::config_dir()?.join("projects");
        let codex = found::codex::configured()?;
        let pi = found::pi::configured()?;
        let flsh = found::flsh::sessions_dir()?;
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

impl Delete {
    /// Refiles a closed conversation in another working folder.
    pub(crate) async fn relocate_session(
        &self,
        agent: &str,
        id: &SessionId,
        destination: &str,
    ) -> anyhow::Result<()> {
        let store = self.sessions.store_of(id);
        let _store = store.lock().await;
        let which: Agent = agent.parse().ok().context("unknown agent")?;
        ensure!(
            matches!(which, Agent::Claude | Agent::Codex | Agent::Pi),
            "this agent cannot move projects yet"
        );
        ensure!(super::saved::valid_id(id.as_ref()), "invalid session id");
        let ours = sessions::find_session(&self.sessions.read(), id);
        let agent_id = match &ours {
            Some(session) => {
                ensure!(
                    session.agent == agent && session.info().state.status == Status::Ended,
                    "close the session before moving it"
                );
                watched::read(&session.live, |live| live.agent_session.clone())
                    .context("session has no agent id")?
            }
            None => id.clone(),
        };
        ensure!(
            self.sessions
                .read()
                .values()
                .filter(|session| session.agent == agent
                    && watched::read(&session.live, |live| {
                        live.agent_session.as_ref() == Some(&agent_id)
                    }))
                .all(|session| session.info().state.status == Status::Ended),
            "close the session before moving it"
        );
        self.check_closed_files(which, &agent_id, ours.is_some())
            .await?;
        let harness = Harness::try_from(which).map_err(|_| anyhow::anyhow!("unsupported agent"))?;
        let file = self
            .listings
            .transcript_file(harness, &agent_id)
            .await
            .context("session transcript not found")?;
        let mut owned = self.sessions.owned();
        owned.remove(&agent_id);
        let old = PathBuf::from(
            self.listings.find_external(which, &agent_id, owned)
                .await?
                .context("session not found")?
                .cwd,
        );
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
                &super::folder::Folders { old, new },
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

        for (key, session) in self.sessions.read().iter() {
            if session.agent == agent
                && (key == id
                    || watched::read(&session.live, |live| {
                        live.agent_session.as_ref() == Some(&agent_id_for_record)
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
