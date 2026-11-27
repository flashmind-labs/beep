//! A session here from a transcript that came from another machine: writes
//! it into the harness's store for the folder it works in here, marks its
//! bytes as counted (the machine that ran it counted them), and starts it
//! through its agent, or keeps it ended when it came ended.

use beep_proto::SessionId;
use super::{Env, Harness, rewrite, store};
use crate::agents::{AgentKind, Begin, SavedSession};
use crate::logs::found::Agent;
use anyhow::{Context, Result};
use beep_proto::{ModelId, node::agent::ParentLink};
use iroh::EndpointId;
use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};
use tokio::task::spawn_blocking;

/// A session to run here.
pub(super) struct Arrival<'a> {
    pub(super) id: &'a SessionId,
    pub(super) mode: Option<String>,
    /// The model it ran on.
    pub(super) model: Option<ModelId>,
    /// The linked worktrees it created that moved with it, as they are here.
    pub(super) worktrees: Vec<String>,
    /// The machine that ran it.
    pub(super) from: EndpointId,
    /// What the source saved of it; `None` from a source from before saved
    /// sessions.
    pub(super) saved: Option<SavedSession>,
    /// A session of a chat, which moves whole: a thread stays its chat's.
    pub(super) chat: bool,
}

use super::folder::Folders;

/// Writes `copy` (a transcript the other machine named `file_name`) into the
/// store for the folder `folders.new`.
pub(super) async fn store(
    env: &Env,
    id: &SessionId,
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
        let (id, folders) = (id.to_owned(), folders.clone());
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
        let (agents, id) = (env.agents.clone(), id.to_owned());
        spawn_blocking(move || agents.spend.mark_counted(agent, &id, &path))
            .await?
            .context("marking the copied usage as counted")?;
    }

    Ok(())
}

/// Starts the session `arrival`, whose transcript is in the store, in
/// `folders.new`.
pub(super) async fn resume(
    env: &Env,
    arrival: &Arrival<'_>,
    kind: &AgentKind,
    folders: &Folders,
) -> Result<()> {
    let cwd = folders.new.to_string_lossy().into_owned();
    let from = env.name_of(&arrival.from);
    let here = env.name_of(&env.mesh.me());
    let note = format!("Moved here from {from}");
    let now = if arrival.chat {
        "its folder, with what it held, is now"
    } else {
        "the same checkout is now"
    };
    let brief = format!(
        "This session moved from another machine, {from}, and now runs on {here}. Paths in the conversation so far name its old folder, {}; {now} {}.",
        folders.old.display(),
        folders.new.display()
    );
    let begin = arrival.begin(note, brief, env.mesh.me());
    let follows = begin
        .parent
        .as_ref()
        .and_then(|link| env.agents.parent_live_of(link));
    let session = env
        .agents
        .start(kind, Some(&cwd), Begin { follows, ..begin })
        .await?;
    env.agents.join_family(&session);
    Ok(())
}

/// What a session that came ended is kept as here: as the source saved it,
/// in `folder`, with a thread's link to its chat pointing here.
pub(super) fn kept_here(arrival: &Arrival<'_>, folder: &Path, me: EndpointId) -> Option<SavedSession> {
    let mut saved = arrival.saved.clone()?;
    saved.info.cwd = folder.to_string_lossy().into_owned();
    saved.info.parent = arrival.parent(me);
    Some(saved)
}

impl Arrival<'_> {
    /// How the session begins here: as the source saved it, with its title,
    /// model, thinking level, persona and whether it is the assistant, and with the
    /// move record's model and mode from a source from before saved sessions.
    pub(super) fn begin(&self, note: String, brief: String, me: EndpointId) -> Begin {
        let (info, effort, fast, named) =
            self.saved.clone().map_or_else(Default::default, |saved| {
                (saved.info, saved.effort, saved.fast, saved.named)
            });

        Begin {
            resume: Some(self.id.clone()),
            parent: self.parent(me),
            named,
            title: info.title,
            mode: info.mode.or_else(|| self.mode.clone()),
            model: info.model.or_else(|| self.model.clone()),
            effort,
            fast,
            worktrees: self.worktrees.clone(),
            assistant: info.assistant,
            persona: info.persona,
            continues: info.continues,
            note: Some(note),
            brief: Some(Cow::Owned(brief)),
            ..Begin::default()
        }
    }

    /// Who started it, for a thread of a chat: its parent as the source
    /// saved it, on this machine `me` when it ran on the source, since the
    /// chat moves whole. `None` for any other session, which leaves the one
    /// that started it behind.
    fn parent(&self, me: EndpointId) -> Option<ParentLink> {
        let link = self.saved.as_ref()?.info.parent.clone().filter(|_| self.chat)?;
        let here = |machine: Option<EndpointId>| machine.filter(|at| *at != self.from).or(Some(me));
        Some(ParentLink {
            machine: here(link.machine),
            root_machine: here(link.root_machine),
            key: link.session.clone(),
            ..link
        })
    }
}
