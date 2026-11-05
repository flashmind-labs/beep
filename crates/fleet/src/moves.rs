//! Moving a coding agent session to another machine: the desktop writes the
//! mesh's `session-move/<id>` record, which both machines act on, and asks
//! the target how it goes.

use crate::{
    desk::Desk,
    error::{FleetError, Result},
    records::{Place, WRITES, place_key, read, scan},
};
use beep_mesh::{
    Key,
    machine::sessions::{
        MoveState, NodeCopies, NodeWork, WorktreeMove, continue_copy, copies, discard_copy,
        move_progress, worktree_move,
    },
};
use beep_proto::{
    AgentSlug,
    node::{
        agent::CopyView,
        mesh_state::{MovedWorktree, SessionMove, StateValue, SyncSpec},
    },
};
use iroh::EndpointId;
use std::{
    path::{Component, Path},
    time::{SystemTime, UNIX_EPOCH},
};

/// A session to move, as the desktop knows it from the source's list.
pub struct MoveAsk<'a> {
    pub agent: &'a str,
    /// The agent's own id.
    pub id: &'a str,
    pub from: EndpointId,
    pub to: EndpointId,
    pub project: &'a str,
    /// The folder the session works in on the source.
    pub cwd: &'a str,
    /// It works in a linked worktree of the project, not its checkout.
    pub worktree: Option<AskedWorktree<'a>>,
    pub mode: Option<String>,
    /// Stop the turn instead of waiting for it.
    pub stop: bool,
}

/// A copy this Mac's node keeps, and how the machine that ran the session
/// is: its name, and whether the mesh has it online now.
pub struct CopyState {
    pub view: CopyView,
    /// `None` for a machine the mesh does not list any more.
    pub from_name: Option<String>,
    pub online: bool,
}

/// The linked worktree a session works in on the source.
pub struct AskedWorktree<'a> {
    /// The worktree's folder.
    pub folder: &'a str,
    /// The branch checked out in it.
    pub branch: &'a str,
}

impl Desk {
    /// Asks `ask.to` to take the session over from `ask.from`. Refused, in
    /// words for the user, when the session is one that cannot move yet.
    pub async fn move_session(&self, ask: MoveAsk<'_>) -> Result<SessionMove> {
        if !matches!(ask.agent, "claude" | "codex" | "pi") {
            return Err(refused(format!(
                "{} sessions cannot move between machines yet",
                ask.agent
            )));
        }
        if ask.from == ask.to {
            return Err(refused("the session is on that machine already".to_owned()));
        }
        let theirs = self
            .project_folder(ask.project, ask.from)
            .ok_or_else(|| refused("the source has no checkout of the project".to_owned()))?;
        if let Some(worktree) = &ask.worktree {
            self.check_worktree_move(&ask, worktree).await?;
        }
        // A worktree session's folder is below the worktree, else below the
        // checkout.
        let base = ask.worktree.as_ref().map_or(theirs.as_str(), |w| w.folder);
        if self.project_folder(ask.project, ask.to).is_none() {
            return Err(refused(
                "that machine has no checkout of the project: import it first".to_owned(),
            ));
        }
        let path = Path::new(ask.cwd)
            .strip_prefix(base)
            .ok()
            .filter(|rest| {
                rest.components()
                    .all(|part| matches!(part, Component::Normal(_)))
            })
            .and_then(Path::to_str)
            .ok_or_else(|| refused("the session works outside the project's checkout".to_owned()))?
            .to_owned();
        let record = SessionMove {
            agent: AgentSlug::from(ask.agent),
            id: ask.id.to_owned(),
            from: ask.from,
            to: ask.to,
            project: ask.project.to_owned(),
            path,
            mode: ask.mode,
            stop: ask.stop,
            at: now_ms(),
            worktree: ask.worktree.as_ref().map(|w| MovedWorktree {
                branch: w.branch.to_owned(),
                from: w.folder.to_owned(),
            }),
        };
        let _one = WRITES.lock().await;
        self.forget_old_moves(record.at).await?;
        self.mesh
            .set(
                Key::SessionMove(ask.id.to_owned()),
                Some(record.to_bytes().map_err(anyhow::Error::from)?),
            )
            .await?;
        Ok(record)
    }

    /// The folder `project` has on `machine`: from its `sync/<name>` record
    /// when that names the machine, else from the machine's place record. A
    /// project imported on one machine at a time has only the place records.
    fn project_folder(&self, project: &str, machine: EndpointId) -> Option<String> {
        read::<SyncSpec>(&self.mesh, &Key::Sync(project.to_owned()))
            .and_then(|mut spec| spec.folders.remove(&machine))
            .or_else(|| {
                read::<Place>(&self.mesh, &place_key(project, machine)).map(|place| place.folder)
            })
    }

    /// Deletes the records of moves that are over by now, so the mesh does
    /// not keep one for every session that ever moved. Done under the write
    /// lock, as the new record is written.
    async fn forget_old_moves(&self, now: u64) -> Result<()> {
        for (key, record) in scan::<SessionMove>(&self.mesh, "session-move/") {
            if now.saturating_sub(record.at) > KEEP_MOVES_MS {
                self.mesh.set(key, None).await?;
            }
        }
        Ok(())
    }

    /// Whether the node of `machine` takes part in moves: a node from before
    /// them does not, and a session must not be sent to it.
    pub async fn takes_moves(&self, machine: EndpointId) -> Result<bool> {
        let client = self.peers.client(machine).await?;
        Ok(!matches!(
            move_progress(&client, PROBE).await?,
            MoveState::NeedsUpdate
        ))
    }

    /// Whether `machine` can take a session working in a worktree on
    /// `branch` of `project`.
    pub async fn worktree_move(
        &self,
        machine: EndpointId,
        project: &str,
        branch: &str,
    ) -> Result<WorktreeMove> {
        let client = self.peers.client(machine).await?;
        Ok(worktree_move(&client, project, branch).await?)
    }

    /// Refuses, before anything stops, a worktree session that either
    /// machine cannot move: a node from before worktree moves, or a
    /// Syncthing set not to sync the worktree.
    async fn check_worktree_move(
        &self,
        ask: &MoveAsk<'_>,
        worktree: &AskedWorktree<'_>,
    ) -> Result<()> {
        if worktree.branch.is_empty() {
            return Err(refused(
                "a session in a worktree with no branch cannot move".to_owned(),
            ));
        }
        for machine in [ask.from, ask.to] {
            // A machine that is away is asked when it returns: the target
            // checks again before it takes the session.
            let Ok(answer) = self
                .worktree_move(machine, ask.project, worktree.branch)
                .await
            else {
                continue;
            };
            match answer {
                WorktreeMove::Takes => {}
                WorktreeMove::NeedsUpdate => {
                    return Err(refused(
                        "update the machines first: one cannot move worktree sessions".to_owned(),
                    ));
                }
                WorktreeMove::Ignored if machine == ask.to => {
                    return Err(refused(
                        "Syncthing is set not to sync this worktree, so it cannot move to that machine"
                            .to_owned(),
                    ));
                }
                WorktreeMove::Ignored => {}
            }
        }
        Ok(())
    }

    /// The copies of other machines' sessions that this Mac's node keeps,
    /// each with the machine that ran it as the mesh says it is now. `None`
    /// for a node from before background copies.
    pub async fn session_copies(&self) -> Result<Option<Vec<CopyState>>> {
        let client = self.peers.client(self.local).await?;
        let NodeCopies::Listed(listed) = copies(&client).await? else {
            return Ok(None);
        };
        let devices = self.mesh.devices();
        Ok(Some(
            listed
                .into_iter()
                .map(|view| {
                    let device = devices.iter().find(|status| status.device.id == view.from);
                    CopyState {
                        from_name: device.map(|status| status.device.name.clone()),
                        online: device.is_some_and(|status| status.online),
                        view,
                    }
                })
                .collect(),
        ))
    }

    /// Has this Mac's node run its copy of a session (or its version set
    /// aside at `lost`) in its own checkout. Refused in the node's words.
    pub async fn continue_copy(&self, agent: &str, id: &str, lost: Option<u64>) -> Result<()> {
        let client = self.peers.client(self.local).await?;
        match continue_copy(&client, agent, id, lost).await? {
            NodeWork::Done(()) => Ok(()),
            NodeWork::Refused(why) => Err(refused(why)),
        }
    }

    /// Has this Mac's node throw a copy away.
    pub async fn discard_copy(&self, agent: &str, id: &str, lost: Option<u64>) -> Result<()> {
        let client = self.peers.client(self.local).await?;
        Ok(discard_copy(&client, agent, id, lost).await?)
    }

    /// Takes back the move of session `id`: the machines stop it unless
    /// the target already resumed the session.
    pub async fn cancel_move(&self, id: &str) -> Result<()> {
        let _one = WRITES.lock().await;
        self.mesh.set(Key::SessionMove(id.to_owned()), None).await?;
        Ok(())
    }

    /// The moves in the mesh, the latest record of each session.
    pub fn moves(&self) -> Vec<SessionMove> {
        scan::<SessionMove>(&self.mesh, "session-move/")
            .into_iter()
            .map(|(_, record)| record)
            .collect()
    }

    /// How the move of `id` stands, as its target says.
    pub async fn move_state(&self, id: &str) -> Result<MoveState> {
        let record: SessionMove =
            read(&self.mesh, &Key::SessionMove(id.to_owned())).ok_or(FleetError::NotFound)?;
        let client = self.peers.client(record.to).await?;
        Ok(move_progress(&client, id).await?)
    }
}

/// How long the record of a move is kept: a session that moved shows once
/// while it is, and a move still waiting for a machine that is away is not
/// lost in a few days.
const KEEP_MOVES_MS: u64 = 30 * 24 * 3600 * 1000;

/// A session id no move has, to ask a node whether it knows moves at all.
const PROBE: &str = "probe";

fn refused(message: String) -> FleetError {
    FleetError::Refused(message)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}
