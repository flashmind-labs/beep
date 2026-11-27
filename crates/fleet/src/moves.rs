//! Moving a coding agent session to another machine: the desktop writes the
//! mesh's `session-move/<id>` record, which both machines act on, and asks
//! the target how it goes.

use crate::{
    desk::Desk,
    error::{FleetError, Result},
    records::{Place, WRITES, now_ms, place_key, read, scan},
    session_titles,
};
use beep_mesh::{
    Key,
    machine::sessions::{MoveState, move_progress},
};
use beep_proto::SessionId;
use beep_proto::{
    AgentSlug, ModelId,
    node::mesh_state::{ConversationMove, SessionMove, StateValue, SyncSpec},
};
use iroh::EndpointId;
use std::{
    path::{Component, Path},
    sync::PoisonError,
    time::Duration,
};
use tokio::time::timeout;

/// How long a move capability probe or progress poll waits on one target machine.
const MOVE_POLL_LIMIT: Duration = Duration::from_millis(1500);

/// A session to move, as the desktop knows it from the source's list.
pub struct MoveAsk<'a> {
    pub agent: &'a str,
    /// The agent's own id.
    pub id: &'a SessionId,
    pub from: EndpointId,
    pub to: EndpointId,
    pub project: &'a str,
    /// The folder the session works in on the source: its project folder
    /// or a folder below it.
    pub cwd: &'a str,
    pub mode: Option<String>,
    /// The model it runs on.
    pub model: Option<ModelId>,
    /// The linked worktrees it created on the source.
    pub worktrees: Vec<String>,
    /// Stop the turn instead of waiting for it.
    pub stop: bool,
}

/// A chat to move with its threads, as the desktop knows them from the
/// source's lists.
pub struct ChatMoveAsk {
    pub from: EndpointId,
    pub to: EndpointId,
    /// The chat, then its threads.
    pub sessions: Vec<ChatSession>,
    /// Stop the turns instead of waiting for them.
    pub stop: bool,
}

/// A session of a chat: the chat itself or one of its threads.
pub struct ChatSession {
    pub agent: String,
    /// The agent's own id.
    pub id: SessionId,
    /// The folder it works in on the source.
    pub cwd: String,
    /// The model it runs on.
    pub model: Option<ModelId>,
    /// It ended, and arrives ended.
    pub ended: bool,
}

impl Desk {
    /// Asks `ask.to` to take a chat over from `ask.from`, with its threads
    /// and its folder: a record for each session, which moves as one of a
    /// project does. Refused, in words for the user, when one of them
    /// cannot move, before anything is written.
    pub async fn move_chat(&self, ask: ChatMoveAsk) -> Result<Vec<SessionMove>> {
        if ask.from == ask.to {
            return Err(FleetError::refused("the chat is on that machine already"));
        }

        let at = now_ms();
        let mut records: Vec<SessionMove> = Vec::with_capacity(ask.sessions.len());

        for session in ask.sessions {
            movable(&session.agent)?;
            let (root, path) = chat_place(&session.cwd).ok_or_else(|| {
                FleetError::refused("only a chat in a folder of its own can move")
            })?;
            let chat = records
                .first()
                .map_or(path.as_str(), |chat| chat.path.as_str());

            if path != chat && !path.starts_with(&format!("{chat}/")) {
                return Err(FleetError::refused("a thread is not in this chat's folder"));
            }

            records.push(SessionMove {
                agent: AgentSlug::from(session.agent.as_str()),
                id: session.id,
                from: ask.from,
                to: ask.to,
                project: String::new(),
                path,
                mode: None,
                stop: ask.stop,
                at,
                worktree: None,
                model: session.model,
                worktrees: Vec::new(),
                conversation: Some(ConversationMove {
                    root: root.to_owned(),
                    ended: session.ended,
                }),
            });
        }

        let _one = WRITES.lock().await;
        self.forget_old_moves(at).await?;

        // The threads first: the chat leaves the folders they bring to them.
        for record in records.iter().rev() {
            self.mesh
                .set(
                    Key::SessionMove(record.id.clone()),
                    Some(record.to_bytes().map_err(anyhow::Error::from)?),
                )
                .await?;
        }

        self.carry_titles(&records).await;
        Ok(records)
    }

    /// Asks `ask.to` to take the session over from `ask.from`. Refused, in
    /// words for the user, when the session is one that cannot move yet.
    pub async fn move_session(&self, ask: MoveAsk<'_>) -> Result<SessionMove> {
        movable(ask.agent)?;

        if ask.from == ask.to {
            return Err(FleetError::refused(
                "the session is on that machine already",
            ));
        }

        let theirs = self
            .project_folder(ask.project, ask.from)
            .ok_or_else(|| FleetError::refused("the source has no checkout of the project"))?;

        if self.project_folder(ask.project, ask.to).is_none() {
            return Err(FleetError::refused(
                "that machine has no checkout of the project: import it first",
            ));
        }

        // The session resumes in the target's project folder, or the same
        // folder below it.
        let path = inside(ask.cwd, &theirs)
            .ok_or_else(|| FleetError::refused("the session works outside the project's checkout"))?
            .to_owned();
        let record = SessionMove {
            agent: AgentSlug::from(ask.agent),
            id: ask.id.clone(),
            from: ask.from,
            to: ask.to,
            project: ask.project.to_owned(),
            path,
            mode: ask.mode,
            stop: ask.stop,
            at: now_ms(),
            // Sessions run in the project folder, never in a worktree.
            worktree: None,
            model: ask.model,
            worktrees: ask.worktrees,
            conversation: None,
        };
        let _one = WRITES.lock().await;
        self.forget_old_moves(record.at).await?;
        self.mesh
            .set(
                Key::SessionMove(ask.id.clone()),
                Some(record.to_bytes().map_err(anyhow::Error::from)?),
            )
            .await?;
        self.carry_titles(std::slice::from_ref(&record)).await;
        Ok(record)
    }

    /// Gives each moving session the title the user gave it on its machine,
    /// which is kept by machine, on the machine it moves to. The move has
    /// started already: a title that does not carry is only logged.
    async fn carry_titles(&self, records: &[SessionMove]) {
        for record in records {
            let (from, to, agent) = (
                record.from.to_string(),
                record.to.to_string(),
                record.agent.to_string(),
            );

            if let Some(title) = session_titles::get(&self.mesh, &from, &agent, &record.id)
                && let Err(e) =
                    session_titles::save(&self.mesh, &to, &agent, &record.id, &title).await
            {
                log::warn!(
                    "carrying the title of {} to its new machine: {e}",
                    record.id
                );
            }
        }
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
                self.hosts
                    .move_states()
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .remove(&record.id);
                self.mesh.set(key, None).await?;
            }
        }

        Ok(())
    }

    /// Whether the node of `machine` takes part in moves: a node from before
    /// them does not, and a session must not be sent to it.
    pub async fn takes_moves(&self, machine: EndpointId) -> Result<bool> {
        let probed = timeout(MOVE_POLL_LIMIT, async {
            let client = self.machines.client(machine).await?;
            Ok::<_, FleetError>(move_progress(&client, &SessionId::from(PROBE)).await?)
        })
        .await
        .map_err(|_| FleetError::Refused("the machine did not answer in time".into()))??;
        Ok(!matches!(probed, MoveState::NeedsUpdate))
    }

    /// Takes back the move of session `id`: the machines stop it unless
    /// the target already resumed the session.
    pub async fn cancel_move(&self, id: &SessionId) -> Result<()> {
        let _one = WRITES.lock().await;
        self.hosts
            .move_states()
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(id);
        self.mesh.set(Key::SessionMove(id.clone()), None).await?;
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
    pub async fn move_state(&self, id: &SessionId) -> Result<MoveState> {
        let record: SessionMove =
            read(&self.mesh, &Key::SessionMove(id.clone())).ok_or(FleetError::NotFound)?;
        let asked = timeout(MOVE_POLL_LIMIT, async {
            let client = self.machines.client(record.to).await?;
            Ok::<_, FleetError>(move_progress(&client, id).await?)
        })
        .await;

        match asked {
            Ok(Ok(state)) => {
                self.hosts
                    .move_states()
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(id.clone(), state.clone());
                Ok(state)
            }
            Ok(Err(err)) => {
                if let Some(cached) = self
                    .hosts
                    .move_states()
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .get(id)
                    .cloned()
                {
                    return Ok(cached);
                }

                Err(err)
            }
            Err(_) => {
                if let Some(cached) = self
                    .hosts
                    .move_states()
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .get(id)
                    .cloned()
                {
                    return Ok(cached);
                }

                Err(FleetError::Refused(
                    "the target machine did not answer in time".into(),
                ))
            }
        }
    }
}

/// Refuses, in words for the user, a session of an agent whose sessions
/// cannot move: one that keeps them in a database.
fn movable(agent: &str) -> Result<()> {
    if matches!(agent, "claude" | "codex" | "pi" | "flsh") {
        return Ok(());
    }

    Err(FleetError::refused(format!(
        "{agent} sessions cannot move between machines yet"
    )))
}

/// Where a chat's session works: the folder its machine keeps its chats in
/// (`<home>/.beep/assistant`), and the session's folder below it with `/`
/// between the names, whichever the machine puts there. `None` for a
/// folder anywhere else.
fn chat_place(cwd: &str) -> Option<(&str, String)> {
    ['/', '\\'].into_iter().find_map(|between| {
        let chats = format!("{between}.beep{between}assistant{between}");
        let at = cwd.rfind(&chats)?;
        let names: Vec<&str> = cwd[at + chats.len()..].split(between).collect();
        let plain = names.iter().all(|name| !matches!(*name, "" | "." | ".."));
        plain.then(|| (&cwd[..at + chats.len() - 1], names.join("/")))
    })
}

/// How long the record of a move is kept: a session that moved shows once
/// while it is, and a move still waiting for a machine that is away is not
/// lost in a few days.
const KEEP_MOVES_MS: u64 = 30 * 24 * 3600 * 1000;

/// A session id no move has, to ask a node whether it knows moves at all.
const PROBE: &str = "probe";

/// `cwd` relative to `base`, when it is `base` or a folder below it. A
/// worktree under `base` (`.worktrees/` or `.claude/worktrees/`) is `base`
/// itself: sessions run in the project folder.
fn inside<'a>(cwd: &'a str, base: &str) -> Option<&'a str> {
    let rest = Path::new(cwd).strip_prefix(base).ok()?;

    if !rest
        .components()
        .all(|part| matches!(part, Component::Normal(_)))
    {
        return None;
    }

    if rest.starts_with(".worktrees") || rest.starts_with(".claude/worktrees") {
        return Some("");
    }

    rest.to_str()
}
