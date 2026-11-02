//! Coding agents (Claude Code, Codex, Gemini CLI, pi, flsh, OpenCode) run over ACP: the
//! table of agents this node can start, the sessions it runs, and the
//! streams the server follows them on (`beep_proto::node::agent`).

use crate::{
    claude::{self, External},
    found::{self, Agent, Stores},
};
use anyhow::{Context, Result};
use beep_proto::node::{
    LinkRx, LinkTx,
    agent::{
        self, Event, ExternalSession, Request, SESSIONS_PROTOCOL, SessionFolder, SessionInfo,
        SessionsReply, SessionsRequest, State, Status,
    },
};
use serde::{Deserialize, Serialize};
use std::{
    cmp::Reverse,
    collections::{BTreeMap, HashMap, HashSet, hash_map::RandomState},
    hash::BuildHasher,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::SystemTime,
};
use tokio::task::spawn_blocking;

mod acp;
mod live;
mod session;

pub(crate) use live::cut;

use acp::Prompt;
use session::{Command, Session, Start};

/// The most sessions running at once.
const MAX_RUNNING: usize = 8;
/// The most sessions kept, ended ones included; the oldest ended go first.
const MAX_KEPT: usize = 64;
/// The most transcript entries one request reads.
const MAX_TRANSCRIPT: u32 = 200;
/// The most folders `Folders` answers, and how many when the server names
/// no number.
const MAX_FOLDERS: usize = 200;
const DEFAULT_FOLDERS: usize = 100;
/// The most sessions `InFolders` answers, and how many by default.
const MAX_IN_FOLDERS: usize = 100;
const DEFAULT_IN_FOLDERS: usize = 50;
/// The most folders one `InFolders` names.
const MAX_ASKED_FOLDERS: usize = 32;

/// An agent this node can start: a program that speaks ACP on its stdio.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentKind {
    /// What Beep calls it, such as `claude`; its protocol is `claude/1`.
    pub name: String,
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Variables set for it on top of the node's own environment.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

/// An agent the daemon knows how to start, when its CLI is installed.
/// Each starts with its own permission prompts off, so it never stops to
/// ask; a permission request that comes anyway is allowed by the session.
struct Known {
    name: &'static str,
    /// The CLI whose presence says the user has this agent.
    cli: &'static str,
    adapter: Adapter,
    /// Variables that turn its permission prompts off.
    env: &'static [(&'static str, &'static str)],
}

enum Adapter {
    /// An ACP adapter from npm: its binary when installed, or run with npx.
    Npm {
        bin: &'static str,
        package: &'static str,
    },
    /// The CLI itself speaks ACP with these arguments.
    Flags(&'static [&'static str]),
}

const KNOWN: &[Known] = &[
    // The session switches it to `bypassPermissions` once it has one.
    Known {
        name: "claude",
        cli: "claude",
        adapter: Adapter::Npm {
            bin: "claude-agent-acp",
            package: "@agentclientprotocol/claude-agent-acp",
        },
        env: &[],
    },
    // Full access: approval policy `never` and sandbox `danger-full-access`,
    // for new and resumed sessions alike.
    Known {
        name: "codex",
        cli: "codex",
        adapter: Adapter::Npm {
            bin: "codex-acp",
            package: "@agentclientprotocol/codex-acp",
        },
        env: &[("INITIAL_AGENT_MODE", "agent-full-access")],
    },
    // YOLO approves every tool. Gemini CLI drops back to `default` in a
    // folder it does not trust, so the folder is trusted too.
    Known {
        name: "gemini",
        cli: "gemini",
        adapter: Adapter::Flags(&["--acp", "--approval-mode", "yolo"]),
        env: &[("GEMINI_CLI_TRUST_WORKSPACE", "true")],
    },
    // pi has no permission prompts; only its extensions ask.
    Known {
        name: "pi",
        cli: "pi",
        adapter: Adapter::Npm {
            bin: "pi-acp",
            package: "pi-acp",
        },
        env: &[],
    },
    // flsh never asks permission.
    Known {
        name: "flsh",
        cli: "flsh",
        adapter: Adapter::Flags(&["acp"]),
        env: &[],
    },
    // Merged over its config as the last rule, which wins over the defaults
    // that ask (`doom_loop`, `external_directory`, reading `.env` files).
    Known {
        name: "opencode",
        cli: "opencode",
        adapter: Adapter::Flags(&["acp"]),
        env: &[("OPENCODE_PERMISSION", r#"{"*":"allow"}"#)],
    },
];

/// The agents installed here, from `KNOWN`, then `extra` from `node.toml`
/// (which replace a known one of the same name).
pub fn detect(extra: &[AgentKind]) -> Vec<AgentKind> {
    let found = |program: &str| which::which(program).is_ok();
    let mut kinds: Vec<AgentKind> = KNOWN
        .iter()
        .filter_map(|known| launch(known, &found))
        .collect();
    for kind in extra {
        kinds.retain(|known| known.name != kind.name);
        kinds.push(kind.clone());
    }
    kinds
}

/// How the node starts `known`, when `found` says its CLI, and the adapter
/// or npx it needs, are installed.
fn launch(known: &Known, found: &dyn Fn(&str) -> bool) -> Option<AgentKind> {
    if !found(known.cli) {
        return None;
    }
    let (program, args) = match &known.adapter {
        Adapter::Npm { bin, .. } if found(bin) => ((*bin).to_owned(), Vec::new()),
        Adapter::Npm { package, .. } if found("npx") => (
            "npx".to_owned(),
            vec!["-y".to_owned(), (*package).to_owned()],
        ),
        Adapter::Npm { .. } => return None,
        Adapter::Flags(flags) => (
            known.cli.to_owned(),
            flags.iter().map(|flag| (*flag).to_owned()).collect(),
        ),
    };
    let env = known
        .env
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect();
    Some(AgentKind {
        name: known.name.to_owned(),
        program,
        args,
        env,
    })
}

/// The agents this node offers and the sessions it runs. Sessions belong
/// to the daemon, so they outlive the streams and the connection.
#[derive(Default)]
pub struct Agents {
    kinds: Vec<AgentKind>,
    sessions: Mutex<HashMap<String, Arc<Session>>>,
    /// Claude Code sessions started outside Beep.
    external: External,
    /// Where Codex, pi, flsh and OpenCode keep their sessions.
    stores: Stores,
}

impl Agents {
    pub fn new(kinds: Vec<AgentKind>) -> Self {
        Self {
            kinds,
            sessions: Mutex::default(),
            external: External::locate(),
            stores: Stores::locate(),
        }
    }

    /// The Claude Code sessions started outside Beep.
    pub fn external(&self) -> &External {
        &self.external
    }

    /// The Claude Code sessions started outside Beep, newest first.
    pub async fn external_sessions(
        &self,
        limit: u32,
    ) -> Result<Vec<beep_proto::node::agent::ExternalSession>> {
        self.external.list(limit, None, self.owned()).await
    }

    /// Every session the agents' stores hold, with the folder it ran in.
    async fn sightings(&self) -> Result<Vec<found::Seen>> {
        let stores = self.stores.clone();
        let others = spawn_blocking(move || stores.sessions(None));
        let (claude, others) = tokio::join!(self.external.sightings(), others);
        let mut seen = claude.unwrap_or_else(|e| {
            log::warn!("Claude Code sessions not read: {e:#}");
            Vec::new()
        });
        seen.extend(others.context("reading the agents' sessions")?);
        Ok(seen)
    }

    /// The checkouts coding agents worked in, newest activity first.
    pub async fn found_folders(&self, limit: u32) -> Result<Vec<SessionFolder>> {
        let limit = match usize::try_from(limit).unwrap_or(MAX_FOLDERS) {
            0 => DEFAULT_FOLDERS,
            limit => limit.min(MAX_FOLDERS),
        };
        let seen = self.sightings().await?;
        let home = dirs::home_dir();
        spawn_blocking(move || found::folders(&seen, home.as_deref(), limit, found::checkout))
            .await
            .context("looking up the folders' checkouts")
    }

    /// The sessions that ran in `folders` or below, newest first.
    pub async fn sessions_in(
        &self,
        folders: Vec<String>,
        limit: u32,
    ) -> Result<Vec<ExternalSession>> {
        let limit = match usize::try_from(limit).unwrap_or(MAX_IN_FOLDERS) {
            0 => DEFAULT_IN_FOLDERS,
            limit => limit.min(MAX_IN_FOLDERS),
        };
        let folders: Vec<PathBuf> = folders
            .into_iter()
            .take(MAX_ASKED_FOLDERS)
            .map(PathBuf::from)
            .filter(|folder| folder.is_absolute())
            .collect();
        let picked = found::in_folders(self.sightings().await?, &folders, limit);
        let (claude, others): (Vec<found::Seen>, Vec<found::Seen>) = picked
            .into_iter()
            .partition(|seen| seen.agent == Agent::Claude);
        let ids: HashSet<String> = claude.into_iter().map(|seen| seen.id).collect();
        let owned = self.owned();
        let stores = self.stores.clone();
        let described = spawn_blocking(move || {
            let now = SystemTime::now();
            others
                .into_iter()
                .map(|seen| stores.describe(seen, now))
                .collect::<Vec<_>>()
        });
        let (claude, others) = tokio::join!(self.external.named(ids, owned.clone()), described);
        let mut sessions = claude?;
        sessions.extend(others.context("reading the sessions' titles")?);
        for session in &mut sessions {
            if session.taken_over.is_none() {
                session.taken_over = owned.get(&session.id).cloned();
            }
        }
        sessions.sort_by_key(|session| Reverse(session.last_activity));
        Ok(sessions)
    }

    /// The sessions this node runs, by the agent's own id, to the node's id.
    fn owned(&self) -> HashMap<String, String> {
        self.lock()
            .values()
            .filter_map(|session| {
                let agent_session = session.live.borrow().agent_session.clone()?;
                Some((agent_session, session.id.clone()))
            })
            .collect()
    }

    /// Takes over the Claude Code session `id` started outside Beep, once
    /// nothing runs it; one this node already runs is returned as it is.
    /// `id` may also be one of this node's sessions that ended: it goes on
    /// from the agent's own session.
    async fn resume(
        &self,
        kind: &AgentKind,
        id: &str,
        prompt: Option<String>,
    ) -> Result<Arc<Session>> {
        let ended = self
            .get(id)
            .map(|session| session.live.borrow().agent_session.clone());
        let id = match ended {
            Some(Some(agent_session)) => agent_session,
            Some(None) => anyhow::bail!("session {id} ended before the agent gave it an id"),
            None => id.to_owned(),
        };
        let id = id.as_str();
        let agent = Agent::from_name(&kind.name).with_context(|| {
            format!(
                "{} sessions started outside Beep cannot be taken over",
                kind.name
            )
        })?;
        let owned = self.owned();
        if let Some(session) = owned.get(id).and_then(|node_id| self.get(node_id))
            && session.live.borrow().status() != Status::Ended
        {
            if let Some(text) = prompt {
                session
                    .order(Command::Prompt(text.into()))
                    .await
                    .map_err(anyhow::Error::msg)?;
            }
            return Ok(session);
        }
        let found = match agent {
            Agent::Claude => self
                .external
                .list(1, Some(id.to_owned()), owned)
                .await?
                .into_iter()
                .next(),
            other => {
                let stores = self.stores.clone();
                let id = id.to_owned();
                spawn_blocking(move || stores.find(other, &id)).await?
            }
        }
        .with_context(|| format!("there is no {} session {id} on this machine", kind.name))?;
        if let Some(reason) = claude::refuse_takeover(&found) {
            anyhow::bail!("{reason}");
        }
        self.start(
            kind,
            Some(&found.cwd),
            Begin {
                prompt,
                resume: Some(id.to_owned()),
            },
        )
    }

    /// Closes every running session and waits until their agents have
    /// stopped: for the node shutting down.
    pub async fn shutdown(&self) {
        let running: Vec<Arc<Session>> = self
            .lock()
            .values()
            .filter(|session| session.live.borrow().status() != Status::Ended)
            .cloned()
            .collect();
        futures::future::join_all(running.iter().map(|session| session.close())).await;
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, Arc<Session>>> {
        self.sessions.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The protocols to offer: one per agent, and `agents/1` when there are any.
    pub fn offers(&self) -> Vec<String> {
        let mut offers: Vec<String> = self
            .kinds
            .iter()
            .map(|kind| agent::protocol(&kind.name))
            .collect();
        if !offers.is_empty() {
            offers.push(SESSIONS_PROTOCOL.to_owned());
        }
        offers
    }

    /// The agent `protocol` names, when this node has it.
    pub fn kind(&self, protocol: &str) -> Option<&AgentKind> {
        let name = agent::agent_of(protocol)?;
        self.kinds.iter().find(|kind| kind.name == name)
    }

    fn get(&self, id: &str) -> Option<Arc<Session>> {
        self.lock().get(id).cloned()
    }

    /// Every session, oldest first.
    fn list(&self) -> Vec<SessionInfo> {
        let mut sessions: Vec<SessionInfo> = self.lock().values().map(|s| s.info()).collect();
        sessions.sort_by_key(|info| info.started_at);
        sessions
    }

    fn start(&self, kind: &AgentKind, cwd: Option<&str>, begin: Begin) -> Result<Arc<Session>> {
        let cwd = folder(cwd)?;
        let mut sessions = self.lock();
        // Checked under the lock: two takeovers of one session must not
        // both start it.
        if let Some(resume) = &begin.resume
            && let Some(running) = sessions.values().find(|session| {
                let live = session.live.borrow();
                live.status() != Status::Ended && live.agent_session.as_ref() == Some(resume)
            })
        {
            anyhow::bail!("session {resume} already runs here as {}", running.id);
        }
        let running = sessions
            .values()
            .filter(|session| session.live.borrow().status() != Status::Ended)
            .count();
        anyhow::ensure!(
            running < MAX_RUNNING,
            "{MAX_RUNNING} sessions already run here; close one first"
        );
        if sessions.len() >= MAX_KEPT {
            let oldest = sessions
                .values()
                .filter(|session| session.live.borrow().status() == Status::Ended)
                .min_by_key(|session| session.live.borrow().info.started_at)
                .map(|session| session.id.clone());
            if let Some(oldest) = oldest {
                sessions.remove(&oldest);
            }
        }
        let id = loop {
            let id = new_id();
            if !sessions.contains_key(&id) {
                break id;
            }
        };
        let resumed = begin.resume.is_some();
        let session = Arc::new(Session::start(Start {
            id: id.clone(),
            kind,
            cwd: &cwd,
            prompt: begin.prompt,
            resume: begin.resume,
        })?);
        let how = if resumed { "took over" } else { "started" };
        log::info!("{how} {} session {id} in {}", kind.name, cwd.display());
        sessions.insert(id, session.clone());
        Ok(session)
    }
}

/// How a session begins: on a prompt, and as a new session or an earlier
/// one resumed by the agent's id.
struct Begin {
    prompt: Option<String>,
    resume: Option<String>,
}

/// The folder a session runs in: `cwd`, with `~` for the home folder, or
/// the home folder itself.
fn folder(cwd: Option<&str>) -> Result<PathBuf> {
    let home = dirs::home_dir();
    let path = match cwd.map(str::trim) {
        None | Some("") | Some("~") => home.context("this machine has no home folder")?,
        Some(rest) if rest.starts_with("~/") => home
            .context("this machine has no home folder")?
            .join(&rest[2..]),
        Some(path) => PathBuf::from(path),
    };
    anyhow::ensure!(
        path.is_absolute(),
        "the folder must be an absolute path, not {}",
        path.display()
    );
    anyhow::ensure!(path.is_dir(), "there is no folder {}", path.display());
    Ok(path)
}

/// Six hex digits, random enough to tell sessions apart.
fn new_id() -> String {
    let random = RandomState::new().hash_one(std::time::SystemTime::now());
    format!("{:06x}", random & 0xff_ffff)
}

/// Serves one agent stream: starts or attaches to a session, then passes
/// orders to it and its changes back until either side ends.
pub async fn serve_session(
    mut tx: LinkTx,
    mut rx: LinkRx,
    agents: Arc<Agents>,
    kind: AgentKind,
) -> Result<()> {
    let first: Request = rx
        .recv()
        .await?
        .context("the stream closed before its first request")?;
    let session = match first {
        Request::Start { cwd, prompt } => agents
            .start(
                &kind,
                cwd.as_deref(),
                Begin {
                    prompt: Some(prompt),
                    resume: None,
                },
            )
            .map_err(|e| format!("{e:#}")),
        Request::Resume { session, prompt } => agents
            .resume(&kind, &session, prompt)
            .await
            .map_err(|e| format!("{e:#}")),
        Request::Attach { session } => match agents.get(&session) {
            Some(found) if found.agent == kind.name => Ok(found),
            Some(found) => Err(format!("session {session} runs {}", found.agent)),
            None => Err(format!("there is no session {session}")),
        },
        other => Err(format!("expected Start, Attach or Resume, got {other:?}")),
    };
    let session = match session {
        Ok(session) => session,
        Err(reason) => return tx.send(&Event::Refused { reason }).await,
    };
    let mut live = session.live.clone();
    let mut seen = Seen::new(&live.borrow_and_update().info);
    tx.send(&Event::Attached {
        session: Box::new(session.info()),
    })
    .await?;
    if seen.state.status == Status::Ended {
        return tx
            .send(&Event::Ended {
                reason: seen.ended(),
            })
            .await;
    }
    loop {
        tokio::select! {
            request = rx.recv::<Request>() => {
                let command = match request {
                    Ok(Some(Request::Prompt { text })) => Command::Prompt(text.into()),
                    Ok(Some(Request::PromptImages { text, images })) => {
                        match Prompt::with_images(text, images) {
                            Ok(prompt) => Command::Prompt(prompt),
                            Err(error) => {
                                tx.send(&Event::Ack { error: Some(error) }).await?;
                                continue;
                            }
                        }
                    }
                    Ok(Some(Request::Answer { question, reply })) => Command::Answer { question, reply },
                    Ok(Some(Request::Cancel {})) => Command::Cancel,
                    Ok(Some(Request::Close {})) => Command::Close,
                    Ok(Some(other)) => {
                        let error = Some(format!("{other:?} is not for an open session"));
                        tx.send(&Event::Ack { error }).await?;
                        continue;
                    }
                    // The session keeps running for the next stream.
                    Ok(None) => return Ok(()),
                    Err(e) => {
                        log::debug!("skipped a request: {e:#}");
                        tx.send(&Event::Ack { error: Some("unknown request".into()) }).await?;
                        continue;
                    }
                };
                let error = session.order(command).await.err();
                tx.send(&Event::Ack { error }).await?;
            }
            changed = live.changed() => {
                let events = seen.events(&live.borrow_and_update());
                let mut ended = false;
                for event in events {
                    ended |= matches!(event, Event::Ended { .. });
                    tx.send(&event).await?;
                }
                if ended {
                    return Ok(());
                }
                if changed.is_err() {
                    let reason = "the session stopped".to_owned();
                    return tx.send(&Event::Ended { reason }).await;
                }
            }
        }
    }
}

/// What a stream has told the server, to send only what changed.
struct Seen {
    state: State,
    question: Option<u32>,
    turns: u32,
}

impl Seen {
    fn new(info: &SessionInfo) -> Self {
        Self {
            state: info.state.clone(),
            question: info.question.as_ref().map(|question| question.id),
            turns: info.turns,
        }
    }

    fn ended(&self) -> String {
        self.state.error.clone().unwrap_or_else(|| "closed".into())
    }

    /// The events that bring the server up to `live`, state first.
    fn events(&mut self, live: &live::Live) -> Vec<Event> {
        let info = &live.info;
        let mut events = Vec::new();
        if info.state != self.state {
            self.state = info.state.clone();
            events.push(Event::State {
                state: self.state.clone(),
            });
        }
        let question = info.question.as_ref().map(|question| question.id);
        if question != self.question {
            self.question = question;
            if let Some(question) = &info.question {
                events.push(Event::Question {
                    question: question.clone(),
                });
            }
        }
        if info.turns > self.turns {
            self.turns = info.turns;
            events.push(Event::TurnDone {
                turn: info.turns,
                stop_reason: live.last_stop.unwrap_or(agent::StopReason::Other),
                last_message: info.last_message.clone(),
            });
        }
        if info.state.status == Status::Ended {
            events.push(Event::Ended {
                reason: self.ended(),
            });
        }
        events
    }
}

/// Serves one `agents/1` request.
pub async fn serve_sessions(mut tx: LinkTx, mut rx: LinkRx, agents: Arc<Agents>) -> Result<()> {
    let Some(request) = rx.recv::<SessionsRequest>().await? else {
        return Ok(());
    };
    let reply = match request {
        SessionsRequest::List {} => SessionsReply::Sessions {
            sessions: agents.list(),
        },
        SessionsRequest::Transcript { session, last } => {
            let last = usize::try_from(last.min(MAX_TRANSCRIPT)).unwrap_or(usize::MAX);
            let read = match agents.get(&session) {
                Some(found) => Ok(Some(found.live.borrow().transcript(last))),
                // A Claude Code session started outside Beep.
                None => agents.external.transcript(&session, last).await,
            };
            match read {
                Ok(Some((entries, earlier))) => SessionsReply::Transcript {
                    entries,
                    earlier: u32::try_from(earlier).unwrap_or(u32::MAX),
                },
                Ok(None) => SessionsReply::Error {
                    message: format!("there is no session {session}"),
                },
                Err(e) => SessionsReply::Error {
                    message: format!("could not read session {session}: {e:#}"),
                },
            }
        }
        SessionsRequest::External {
            agent,
            limit,
            session,
        } if agent == "claude" => {
            match agents.external.list(limit, session, agents.owned()).await {
                Ok(sessions) => SessionsReply::External { sessions },
                Err(e) => SessionsReply::Error {
                    message: format!("{e:#}"),
                },
            }
        }
        SessionsRequest::External { agent, .. } => SessionsReply::Error {
            message: format!("this node lists no {agent} sessions started outside Beep"),
        },
        SessionsRequest::Folders { limit } => match agents.found_folders(limit).await {
            Ok(folders) => SessionsReply::Folders { folders },
            Err(e) => SessionsReply::Error {
                message: format!("{e:#}"),
            },
        },
        SessionsRequest::InFolders { folders, limit } => {
            match agents.sessions_in(folders, limit).await {
                Ok(sessions) => SessionsReply::External { sessions },
                Err(e) => SessionsReply::Error {
                    message: format!("{e:#}"),
                },
            }
        }
        SessionsRequest::Watch { sessions } => {
            return agents.external.watch(tx, rx, sessions).await;
        }
        other => SessionsReply::Error {
            message: format!("this node does not serve {other:?}"),
        },
    };
    tx.send(&reply).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extra_agents_replace_known_ones() {
        let custom = AgentKind {
            name: "claude".into(),
            program: "/opt/acp/claude".into(),
            args: vec![],
            env: BTreeMap::new(),
        };
        let kinds = detect(std::slice::from_ref(&custom));
        assert_eq!(kinds.iter().filter(|kind| kind.name == "claude").count(), 1);
        assert!(kinds.contains(&custom));
    }

    #[test]
    fn pi_and_flsh_are_known() {
        let known = |name: &str| KNOWN.iter().find(|known| known.name == name);
        assert!(matches!(
            known("pi").map(|known| &known.adapter),
            Some(Adapter::Npm { bin: "pi-acp", .. })
        ));
        assert!(matches!(
            known("flsh").map(|known| &known.adapter),
            Some(Adapter::Flags(["acp"]))
        ));
    }

    #[test]
    fn opencode_runs_its_own_acp_server() {
        let known = KNOWN
            .iter()
            .find(|known| known.name == "opencode")
            .expect("opencode is known");
        let installed = |program: &str| program == "opencode";
        assert_eq!(
            launch(known, &installed),
            Some(AgentKind {
                name: "opencode".into(),
                program: "opencode".into(),
                args: vec!["acp".into()],
                env: BTreeMap::from([(
                    "OPENCODE_PERMISSION".to_owned(),
                    r#"{"*":"allow"}"#.to_owned()
                )]),
            })
        );
        assert_eq!(launch(known, &|_: &str| false), None);
        assert_eq!(Agent::from_name("opencode"), Some(Agent::OpenCode));
    }

    #[test]
    fn every_known_agent_starts_without_permission_prompts() {
        let launched = |name: &str| {
            let known = KNOWN
                .iter()
                .find(|known| known.name == name)
                .expect("a known agent");
            let kind = launch(known, &|_: &str| true).expect("installed");
            let env: Vec<(String, String)> = kind.env.into_iter().collect();
            (kind.args, env)
        };
        let pair = |name: &str, value: &str| (name.to_owned(), value.to_owned());
        // Claude Code is switched to bypassPermissions by the session.
        assert_eq!(launched("claude"), (vec![], vec![]));
        assert_eq!(
            launched("codex"),
            (
                vec![],
                vec![pair("INITIAL_AGENT_MODE", "agent-full-access")]
            )
        );
        assert_eq!(
            launched("gemini"),
            (
                vec!["--acp".into(), "--approval-mode".into(), "yolo".into()],
                vec![pair("GEMINI_CLI_TRUST_WORKSPACE", "true")]
            )
        );
        assert_eq!(
            launched("opencode"),
            (
                vec!["acp".into()],
                vec![pair("OPENCODE_PERMISSION", r#"{"*":"allow"}"#)]
            )
        );
        // Neither has permission prompts to turn off.
        assert_eq!(launched("pi"), (vec![], vec![]));
        assert_eq!(launched("flsh"), (vec!["acp".into()], vec![]));
        let rules: serde_json::Value =
            serde_json::from_str(&launched("opencode").1[0].1).expect("JSON");
        assert_eq!(rules, serde_json::json!({"*": "allow"}));
    }

    #[test]
    fn node_toml_agents_may_set_variables() {
        let kind: AgentKind = toml::from_str(
            "name = \"codex\"\nprogram = \"/opt/acp/codex-acp\"\n\
             env = { INITIAL_AGENT_MODE = \"agent-full-access\" }",
        )
        .expect("an agent");
        assert_eq!(
            kind.env.get("INITIAL_AGENT_MODE").map(String::as_str),
            Some("agent-full-access")
        );
        let plain: AgentKind = toml::from_str("name = \"x\"\nprogram = \"x\"").expect("an agent");
        assert!(plain.env.is_empty());
        assert!(!toml::to_string(&plain).expect("toml").contains("env"));
    }

    #[test]
    fn an_npm_adapter_runs_its_binary_else_npx() {
        let known = KNOWN
            .iter()
            .find(|known| known.name == "pi")
            .expect("pi is known");
        let launched = |installed: &[&str]| {
            let installed = |program: &str| installed.contains(&program);
            launch(known, &installed).map(|kind| (kind.program, kind.args))
        };
        assert_eq!(launched(&["pi", "pi-acp"]), Some(("pi-acp".into(), vec![])));
        assert_eq!(
            launched(&["pi", "npx"]),
            Some(("npx".into(), vec!["-y".into(), "pi-acp".into()]))
        );
        assert_eq!(launched(&["pi"]), None);
    }

    #[test]
    fn offers_name_each_agent_and_the_session_list() {
        let agents = Agents::new(vec![AgentKind {
            name: "fake".into(),
            program: "true".into(),
            args: vec![],
            env: BTreeMap::new(),
        }]);
        assert_eq!(agents.offers(), ["fake/1", SESSIONS_PROTOCOL]);
        assert!(agents.kind("fake/1").is_some());
        assert!(agents.kind("claude/1").is_none());
        assert!(Agents::new(vec![]).offers().is_empty());
    }

    #[test]
    fn a_folder_must_exist_and_be_absolute() {
        assert!(folder(Some(if cfg!(windows) { "C:\\" } else { "/" })).is_ok());
        assert!(folder(None).is_ok());
        assert!(folder(Some("relative/dir")).is_err());
        assert!(folder(Some("/no/such/folder/here")).is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn one_session_cannot_be_taken_over_twice() {
        // An adapter that never answers: the session stays starting.
        let kind = AgentKind {
            name: "claude".into(),
            program: "sh".into(),
            args: vec!["-c".into(), "cat > /dev/null".into()],
            env: BTreeMap::new(),
        };
        let agents = Agents::new(vec![kind.clone()]);
        let id = "0f3c2a9e-1111-4222-8333-944455556666";
        let begin = || Begin {
            prompt: None,
            resume: Some(id.to_owned()),
        };
        let first = agents.start(&kind, Some("/"), begin()).unwrap();
        let second = agents.start(&kind, Some("/"), begin()).err().unwrap();
        assert!(
            second
                .to_string()
                .contains(&format!("already runs here as {}", first.id)),
            "{second}"
        );
        assert_eq!(agents.owned().get(id), Some(&first.id));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn an_ended_node_session_resumes_from_the_agents_session() {
        let kind = AgentKind {
            name: "claude".into(),
            program: "sh".into(),
            args: vec!["-c".into(), "cat > /dev/null".into()],
            env: BTreeMap::new(),
        };
        let agents = Agents::new(vec![kind.clone()]);
        let id = "0f3c2a9e-1111-4222-8333-944455556666";
        let begin = Begin {
            prompt: None,
            resume: Some(id.to_owned()),
        };
        let first = agents.start(&kind, Some("/"), begin).unwrap();
        first.close().await;
        // Nothing on this machine has the agent's session, so it is the
        // agent's id that the node looks for, not its own.
        let err = agents.resume(&kind, &first.id, None).await.err().unwrap();
        assert!(err.to_string().contains(id), "{err}");
    }

    #[test]
    fn ids_are_six_hex_digits() {
        let id = new_id();
        assert_eq!(id.len(), 6);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
