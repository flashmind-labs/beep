//! Running a session here from a transcript that came from another machine:
//! writes it into the harness's store for the folder it works in here, marks
//! its bytes as counted (the machine that ran it counted them), and starts
//! it through its agent. A session moved here by its source.

use super::{Env, Harness, rewrite, store};
use crate::agents::{AgentKind, Begin, found::Agent};
use anyhow::{Context, Result};
use beep_proto::ModelId;
use iroh::EndpointId;
use std::{borrow::Cow, path::PathBuf};
use tokio::task::spawn_blocking;

/// A session to run here.
pub(super) struct Arrival<'a> {
    pub(super) id: &'a str,
    pub(super) mode: Option<String>,
    /// The model it ran on.
    pub(super) model: Option<ModelId>,
    /// The machine that ran it.
    pub(super) from: EndpointId,
}

use super::folder::Folders;

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
    // Refused before a file is written: a full node would otherwise set the
    // session's transcript aside and then fail to start it.
    let agent = kind.name.parse::<Agent>().ok();
    let harness = agent
        .and_then(|agent| Harness::try_from(agent).ok())
        .with_context(|| format!("{} sessions cannot move yet", kind.name))?;
    let stored = {
        let id = arrival.id.to_owned();
        let folders = folders.clone();
        spawn_blocking(move || -> Result<_> {
            let bytes = std::fs::read(&copy)?;
            // A copy cut off mid-line (a pull that was interrupted) ends at
            // its last whole line.
            let whole = bytes
                .iter()
                .rposition(|b| *b == b'\n')
                .map_or(0, |at| at + 1);
            Ok(rewrite(harness, &bytes[..whole], &id, &file_name, &folders))
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
    let note = format!("Moved here from {from}");
    let brief = format!(
        "This session moved from another machine. Paths in the conversation so far name its old folder, {}; the same checkout is now {}.",
        folders.old.display(),
        folders.new.display()
    );
    env.agents.start(
        kind,
        Some(&cwd),
        Begin {
            resume: Some(arrival.id.to_owned()),
            mode: arrival.mode.clone(),
            model: arrival.model.clone(),
            note: Some(note),
            brief: Some(Cow::Owned(brief)),
            ..Begin::default()
        },
    )?;
    Ok(())
}
