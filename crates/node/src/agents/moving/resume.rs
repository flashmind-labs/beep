//! Running a session here from a transcript that came from another machine:
//! writes it into the harness's store for the folder it works in here, marks
//! its bytes as counted (the machine that ran it counted them), and starts
//! it through its agent. A session moved by its source, or continued by the
//! user from a background copy.

use super::{Env, Harness, cache::Meta, offers::harness_of, rewrite, store};
use crate::agents::{AgentKind, Begin, found::Agent};
use anyhow::{Context, Result, bail};
use beep_mesh::Key;
use beep_proto::{
    AgentSlug,
    node::mesh_state::{StateValue, SyncSpec},
};
use iroh::EndpointId;
use std::{
    borrow::Cow,
    path::{Component, Path, PathBuf},
};
use tokio::task::spawn_blocking;

/// How the session came to run here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum How {
    /// Its source ended it for a move.
    Moved,
    /// The user continued it from a copy.
    Continued,
}

/// A session to run here.
pub(super) struct Arrival<'a> {
    pub(super) id: &'a str,
    pub(super) mode: Option<String>,
    /// The machine that ran it.
    pub(super) from: EndpointId,
    pub(super) how: How,
}

/// The folder the session worked in on the other machine and the folder it
/// works in here.
pub(super) struct Folders {
    pub(super) old: PathBuf,
    pub(super) new: PathBuf,
}

/// Writes `copy` (a transcript the other machine named `file_name`) into the
/// store and starts the session in `folders.new`.
pub(super) async fn resume(
    env: &Env,
    arrival: &Arrival<'_>,
    kind: &AgentKind,
    folders: &Folders,
    file_name: String,
    copy: PathBuf,
) -> Result<()> {
    let agent = Agent::from_name(&kind.name);
    let harness = agent
        .and_then(Harness::of)
        .with_context(|| format!("{} sessions cannot move yet", kind.name))?;
    let stored = {
        let (id, old, new) = (
            arrival.id.to_owned(),
            folders.old.clone(),
            folders.new.clone(),
        );
        spawn_blocking(move || -> Result<_> {
            let bytes = std::fs::read(&copy)?;
            // A copy cut off mid-line (a pull that was interrupted) ends at
            // its last whole line.
            let whole = bytes
                .iter()
                .rposition(|b| *b == b'\n')
                .map_or(0, |at| at + 1);
            Ok(rewrite(
                harness,
                &bytes[..whole],
                &id,
                &file_name,
                &old,
                &new,
            ))
        })
        .await??
    };
    let path = env.roots.path(harness, &stored);
    let at = path.clone();
    spawn_blocking(move || store::write(&at, &stored)).await??;
    if let Some(agent) = agent {
        let (agents, id) = (env.agents.clone(), arrival.id.to_owned());
        spawn_blocking(move || agents.spend.mark_counted(agent, &id, &path))
            .await?
            .context("marking the copied usage as counted")?;
    }
    let cwd = folders.new.to_string_lossy().into_owned();
    let from = env.name_of(&arrival.from);
    let (note, brief) = match arrival.how {
        How::Moved => (
            format!("Moved here from {from}"),
            format!(
                "This session moved from another machine. Paths in the conversation so far name its old folder, {}; the same checkout is now {}.",
                folders.old.display(),
                folders.new.display()
            ),
        ),
        How::Continued => (
            format!("Continued here from a copy made on {from}"),
            format!(
                "This session continues from a copy of another machine's, which may not have its latest turns. Paths in the conversation so far name its old folder, {}; the same checkout is now {}.",
                folders.old.display(),
                folders.new.display()
            ),
        ),
    };
    env.agents.start(
        kind,
        Some(&cwd),
        Begin {
            resume: Some(arrival.id.to_owned()),
            mode: arrival.mode.clone(),
            note: Some(note),
            brief: Some(Cow::Owned(brief)),
            ..Begin::default()
        },
    )?;
    Ok(())
}

/// Continues the session `id` of `agent` here from its background copy; the
/// copy goes once the session runs. Never touches the machine that ran it.
pub(super) async fn continue_here(
    env: &Env,
    agent: &AgentSlug,
    id: &str,
    lost: Option<u64>,
) -> Result<()> {
    if harness_of(agent.as_str()).is_none() {
        bail!("{agent} sessions cannot be continued yet");
    }
    if let Some(at) = lost {
        let (cache, slug, key) = (env.cache.clone(), agent.clone(), id.to_owned());
        spawn_blocking(move || cache.promote_lost(&slug, &key, at))
            .await?
            .with_context(|| format!("session {id} has no such version set aside"))?;
    }
    let (copy, meta) = {
        let (cache, slug, key) = (env.cache.clone(), agent.clone(), id.to_owned());
        spawn_blocking(move || Some((cache.copy(&slug, &key)?, cache.meta(&slug, &key)?)))
            .await?
            .with_context(|| format!("this machine has no copy of session {id}"))?
    };
    if env.agents.runs(id) {
        bail!("session {id} runs on this machine already");
    }
    let kind = env
        .agents
        .kinds
        .iter()
        .find(|kind| kind.name == agent.as_str())
        .with_context(|| format!("this machine has no {agent}"))?
        .clone();
    let folders = continued_folders(env, &meta)?;
    if !folders.new.is_dir() {
        bail!(
            "{} has no folder here to continue in: sync the project first",
            meta.project
        );
    }
    let arrival = Arrival {
        id,
        mode: None,
        from: meta.from,
        how: How::Continued,
    };
    resume(env, &arrival, &kind, &folders, meta.file_name.clone(), copy).await?;
    // The copy goes, and a note stays of the point it was taken at, so a
    // later session of another machine that went on from there is known.
    let (cache, agent, id) = (env.cache.clone(), agent.clone(), id.to_owned());
    spawn_blocking(move || {
        cache.remove(&agent, &id, false);
        cache.mark_continued(&agent, &id, meta.last_activity)
    })
    .await?
    .context("noting that the session continues here")?;
    Ok(())
}

/// The folder a copied session worked in on its machine and the same
/// folder here, from the project's `sync/<name>` record.
fn continued_folders(env: &Env, meta: &Meta) -> Result<Folders> {
    let bytes = env
        .mesh
        .get(&Key::Sync(meta.project.clone()))
        .with_context(|| format!("{} is not a synced project", meta.project))?;
    let spec = SyncSpec::from_bytes(&bytes).context("the project's record does not read")?;
    let theirs = spec
        .folders
        .get(&meta.from)
        .context("the machine that ran it does not keep the project")?;
    let mine = spec
        .folders
        .get(&env.mesh.me())
        .context("this machine does not keep the project")?;
    let relative = Path::new(&meta.path);
    if relative
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        bail!("the session's folder is not inside the project");
    }
    Ok(Folders {
        old: Path::new(theirs).join(relative),
        new: Path::new(mine).join(relative),
    })
}
