//! Coding agents on the machines of the mesh, asked directly: the sessions,
//! starting, steering and answering them, their pictures and files, and
//! the harnesses and sign-ins of each machine.

use super::{
    Fleet,
    asked::{BEFORE_ACCOUNTS, BEFORE_ASSISTANT, BEFORE_CHAT_MOVES, newer_than},
    view_signal::{Change, ring},
};
use crate::error::{DesktopError, Result};
use beep_mesh::{
    Key, MeshError, PeerClient,
    machine::{
        self, Host, Makes, OpenFailed, Opened as Attached,
        auto::AutoHost,
        codex_login, ended,
        listing::{self, Linked},
        open_agent, open_agent_staged, order,
        sessions::{self, NodeFile, NodeWork, Picture},
    },
};
use beep_proto::SessionId;
use beep_proto::{
    AGENT_FILE_CHUNK, AccountName, AgentFileChunk, AgentHarness, AgentHarnesses, AgentHost,
    AgentImageData, AgentModels, AgentSessionModes, AgentSessionTasks, AgentSessionView, ApiError,
    CodexLogin, CodexLoginState, CustomAgent, GetAgentFile, GetAgentImage, GetAgentSession,
    GetAgentSessionModes, GetAgentSessionTasks, HarnessEnv, HarnessSettings, HarnessSignIn,
    ListAgentModels, MachineSession, ResumeAgentSession, StartAgentSession, StartedAgent,
    SteerAgentSession, Upload, agent_name, image_key, media_mime,
    node::{
        agent::{self, AgentCommand, Request, WorkPlace, Worktree},
        harness::{HarnessConfig, HarnessInfo, harness},
        mesh_state::StateValue,
    },
};
use futures::{StreamExt, stream};
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    future::Future,
    sync::Arc,
    time::{Duration, UNIX_EPOCH},
};
use tauri::State;
use tokio::time::{error::Elapsed, timeout};

/// A machine that can run a coding agent.
#[derive(Debug, Serialize)]
pub struct HostRow {
    #[serde(flatten)]
    host: AgentHost,
    /// Its node runs the home assistant.
    assistant: bool,
    /// Its node takes part in moving a chat.
    moves_chats: bool,
}

/// The machines that can run an agent, without a harness the desktops switched off.
#[tauri::command]
pub async fn agent_hosts(fleet: State<'_, Arc<Fleet>>) -> Result<Vec<HostRow>> {
    let hosts = fleet.status().agent_hosts(|agent| fleet.harness_off(agent));

    Ok(hosts
        .into_iter()
        .map(|host| HostRow {
            assistant: runs_assistant(&fleet, &host),
            moves_chats: fleet
                .member(&host.machine_id)
                .is_some_and(|machine| moves_chats(&fleet, machine)),
            host,
        })
        .collect())
}

/// The most transcript entries one view reads at once.
const MAX_ENTRIES: u32 = 200;
/// Machines asked at once.
const IN_FLIGHT: usize = 4;
/// How long an order to a session may take.
const ORDER_WAIT: Duration = Duration::from_secs(10);
/// How long a closed session gets to stop: the node gives its agent two
/// seconds before it kills it.
const CLOSE_WAIT: Duration = Duration::from_secs(10);

fn not_found() -> DesktopError {
    DesktopError::Api(ApiError::NotFound {})
}

/// `call`, given up after `listing::NODE_LIMIT`.
async fn limited<T>(call: impl Future<Output = std::result::Result<T, MeshError>>) -> Result<T> {
    match timeout(listing::NODE_LIMIT, call).await {
        Ok(done) => Ok(done?),
        Err(e) => Err(DesktopError::Mesh(MeshError::Link(
            anyhow::Error::new(e).context("the machine did not answer in time"),
        ))),
    }
}

impl Fleet {
    pub(crate) fn host_of(&self, machine: EndpointId) -> Host {
        Host {
            id: machine.to_string(),
            name: self.name_of(machine),
        }
    }

    /// Whether the user turned `agent` off, in the mesh's shared settings.
    fn harness_off(&self, agent: &str) -> bool {
        self.mesh()
            .and_then(|mesh| mesh.get(&Key::Harness(agent.to_owned())))
            .and_then(|bytes| HarnessConfig::from_bytes(&bytes).ok())
            .is_some_and(|config| !config.enabled)
    }

    /// `machine` as a place to run agents; `None` when it offers none now.
    pub(crate) fn agent_host(&self, machine: EndpointId) -> Option<AgentHost> {
        hosts(self)
            .into_iter()
            .find(|host| host.machine_id == machine.to_string())
    }
}

/// The online machines of the mesh that offer an agent, as their last
/// status reports say.
pub(crate) fn hosts(fleet: &Fleet) -> Vec<AgentHost> {
    let mut found = fleet.status().agent_hosts(|agent| fleet.harness_off(agent));
    found.sort_by(|a, b| a.machine_name.cmp(&b.machine_name));
    found
}

/// How long one machine gets, all told, to answer a listing: its
/// connection, then its answer.
const MACHINE_LIMIT: Duration = Duration::from_secs(5);
/// How long a scan of what a machine's agents keep may take.
const SCAN_LIMIT: Duration = Duration::from_secs(8);

/// Beep's sessions on `machine` alone, children and reviewers with them, for
/// its answer to replace only its rows; `None` when it could not be listed,
/// so the rows held stay.
pub(crate) async fn sessions_of(fleet: &Fleet, machine: EndpointId) -> Option<Vec<Linked>> {
    let client = fleet.client(machine).await.ok()?;
    let host = fleet.host_of(machine);

    timeout(
        MACHINE_LIMIT,
        listing::listed_sessions(&client, &host, true),
    )
    .await
    .ok()?
}

/// Beep's sessions on `machine` as it pushed them, in the shape `sessions_of`
/// answers; `None` when `value` is not a list of sessions.
pub(crate) fn pushed_sessions(
    fleet: &Fleet,
    machine: EndpointId,
    value: &beep_proto::node::events::Value,
) -> Option<Vec<Linked>> {
    let beep_proto::node::events::Value::Sessions(reply) = value else {
        return None;
    };
    let agent::SessionsReply::Sessions { sessions } = reply.as_ref() else {
        return None;
    };
    let host = fleet.host_of(machine);

    Some(
        sessions
            .iter()
            .map(|info| Linked {
                parent: info.parent.clone(),
                reviewer: info.reviewer,
                session: listing::beep_session(&host, info.clone()),
            })
            .collect(),
    )
}

/// What only a scan of `machine` finds: the sessions started outside Beep
/// that run now, and the assistant's conversations that ended, with their
/// threads. `None` when it did not answer in time.
pub(crate) async fn scanned_sessions_of(
    fleet: &Fleet,
    machine: EndpointId,
) -> Option<Vec<MachineSession>> {
    let scan = async {
        let client = fleet.client(machine).await.ok()?;
        let host = fleet.host_of(machine);
        let home = fleet
            .status()
            .last_env(machine)
            .map(|env| env.home)
            .filter(|home| !home.is_empty());
        let ended = async {
            match &home {
                Some(home) => listing::ended_conversations(&client, &host, home).await,
                None => Vec::new(),
            }
        };
        let (mut found, ended) = tokio::join!(listing::outside_sessions(&client, &host), ended);
        found.extend(ended);
        Some(found)
    };

    timeout(SCAN_LIMIT, scan).await.ok().flatten()
}

/// One session in full; a transcript that cannot be read leaves `entries` empty.
pub(crate) async fn session(
    fleet: &Fleet,
    machine: EndpointId,
    get: &GetAgentSession,
) -> Result<AgentSessionView> {
    let client = fleet.client(machine).await?;
    let host = fleet.host_of(machine);
    // The transcript is asked for alongside the list: one wait, not two.
    // A long log on a busy node takes longer than a listing, so its read
    // keeps its own limit, within the caller's.
    let last = get.last.clamp(1, MAX_ENTRIES);
    let (listed, read) = tokio::join!(
        limited(sessions::list_all(&client)),
        sessions::transcript_before(&client, &get.session, last, true, get.before),
    );
    let mut view = match listed?
        .into_iter()
        .find(|info| info.id == get.session && info.agent == get.agent)
    {
        Some(info) => machine::beep_view(&host, info),
        None => {
            // A node that cannot list outside sessions has none to find.
            let outside = limited(sessions::external(
                &client,
                &get.agent,
                1,
                Some(get.session.clone()),
            ))
            .await
            .unwrap_or_default();
            let found = outside
                .into_iter()
                .find(|session| session.id == get.session)
                .ok_or_else(not_found)?;
            let last_message = found.last_message.clone();
            AgentSessionView {
                model: None,
                session: machine::external_session(&host, found),
                plan: Vec::new(),
                files: Vec::new(),
                last_message,
                error: None,
                queued: 0,
                queued_messages: Vec::new(),
                turns: 0,
                entries: Vec::new(),
                earlier: 0,
                last_stop: None,
                usage: None,
            }
        }
    };

    match read {
        Ok((entries, earlier)) => {
            view.entries = machine::entries_for(entries, true);
            view.earlier = earlier;
        }
        Err(e) => {
            log::warn!("transcript not read; session={} error={e:#}", get.session);
            // A session started outside Beep shows why its log is empty. A
            // Beep session's read fails instead: an empty log would replace
            // the one the window holds, and the next read draw it all again.
            if !view.session.external {
                return Err(e.into());
            }

            view.error = Some(format!("{e:#}"));
        }
    }

    Ok(view)
}

/// The `last` entries before entry `before` of one session, without listing the sessions.
pub(crate) async fn older(
    fleet: &Fleet,
    machine: EndpointId,
    get: &GetAgentSession,
) -> Result<(Vec<beep_proto::AgentEntry>, u32)> {
    let client = fleet.client(machine).await?;
    let last = get.last.clamp(1, MAX_ENTRIES);
    let (entries, earlier) = limited(sessions::transcript_before(
        &client,
        &get.session,
        last,
        true,
        get.before,
    ))
    .await?;
    Ok((machine::entries_for(entries, true), earlier))
}

/// The session `session` of `agent` as the node lists it now.
async fn info(client: &PeerClient, agent: &str, session: &SessionId) -> Result<agent::SessionInfo> {
    limited(sessions::list_all(client))
        .await?
        .into_iter()
        .find(|info| info.id == *session && info.agent == agent)
        .ok_or_else(not_found)
}

/// The device session `session` of `agent` uses, as its node keeps it.
pub(crate) async fn device(
    fleet: &Fleet,
    machine: EndpointId,
    agent: &str,
    session: &SessionId,
) -> Result<Option<agent::SessionDevice>> {
    let client = fleet.client(machine).await?;
    Ok(info(&client, agent, session).await?.device)
}

/// The linked worktrees session `session` of `agent` created, as its node
/// keeps them; empty when the source is offline or does not list the session,
/// and an error when a connected source fails or times out answering so a move
/// never silently drops existing worktrees.
pub(crate) async fn created_worktrees(
    fleet: &Fleet,
    machine: EndpointId,
    agent: &str,
    session: &SessionId,
) -> Result<Vec<String>> {
    let probe = async {
        let client = match fleet.client(machine).await {
            Ok(client) => client,
            Err(DesktopError::Offline(_)) => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };

        match info(&client, agent, session).await {
            Ok(info) => Ok(info.worktrees),
            Err(DesktopError::Api(ApiError::NotFound {})) => Ok(Vec::new()),
            Err(e) => Err(DesktopError::TimedOut(format!(
                "Could not read the worktrees of session {session} from {}: {e}",
                fleet.name_of(machine)
            ))),
        }
    };

    timeout(Duration::from_secs(4), probe).await.map_err(|_| {
        DesktopError::TimedOut(format!(
            "Could not read the worktrees of session {session} from {}: did not answer in time",
            fleet.name_of(machine)
        ))
    })?
}

pub(crate) async fn modes(
    fleet: &Fleet,
    machine: EndpointId,
    get: &GetAgentSessionModes,
) -> Result<AgentSessionModes> {
    let client = fleet.client(machine).await?;
    let info = info(&client, &get.agent, &get.session).await?;
    Ok(AgentSessionModes {
        current: info.mode,
        modes: info
            .modes
            .into_iter()
            .map(|mode| beep_proto::AgentMode {
                id: mode.id,
                name: mode.name,
            })
            .collect(),
        plan: info.plan_ready,
    })
}

pub(crate) async fn tasks(
    fleet: &Fleet,
    machine: EndpointId,
    get: &GetAgentSessionTasks,
) -> Result<AgentSessionTasks> {
    let client = fleet.client(machine).await?;
    let info = info(&client, &get.agent, &get.session).await?;
    Ok(machine::session_tasks(info.state.tasks))
}

/// A session on `machine` named by the node, as a start gives it back.
fn started(host: &Host, info: agent::SessionInfo, why: String) -> StartedAgent {
    StartedAgent {
        session: machine::beep_session(host, info),
        why,
    }
}

/// Starts a session, with `images` sent after the prompt; with no machine
/// named, Auto picks among the mesh's.
pub(crate) async fn start(
    fleet: &Fleet,
    start: StartAgentSession,
    images: Vec<Upload>,
) -> Result<StartedAgent> {
    let planned = plan(fleet, start, images).await?;
    Ok(open(fleet, planned).await?)
}

/// A start checked and ready for its machine: where it goes and what the
/// node is sent.
#[derive(Debug)]
pub(crate) struct Planned {
    pub(crate) machine: EndpointId,
    pub(crate) agent: String,
    /// A `Request::Start`.
    pub(crate) first: Request,
    /// Why Auto picked the machine; empty when the start named it.
    pub(crate) why: String,
    /// The named account `first` runs as, sent as `Request::StartAs`.
    pub(crate) account: Option<AccountName>,
}

/// Checks `start` and builds what its machine is sent. Only Auto, when no
/// machine is named, asks the machines; a named machine need not be on: it
/// is held to the agents it last said it offers, or, before it said, to the
/// agent named.
pub(crate) async fn plan(
    fleet: &Fleet,
    start: StartAgentSession,
    images: Vec<Upload>,
) -> Result<Planned> {
    plan_within(fleet, start, images, Duration::from_secs(5)).await
}

/// Like [`plan`], bounding Auto's per-machine probes tightly so starting a session never blocks
/// on a degraded node before returning its `start:<uuid>` handle.
pub(crate) async fn plan_fast(
    fleet: &Fleet,
    start: StartAgentSession,
    images: Vec<Upload>,
) -> Result<Planned> {
    plan_within(fleet, start, images, Duration::from_millis(500)).await
}

async fn plan_within(
    fleet: &Fleet,
    start: StartAgentSession,
    images: Vec<Upload>,
    limit: Duration,
) -> Result<Planned> {
    let prompt = start.prompt.trim().to_owned();

    if prompt.is_empty() && images.is_empty() {
        return Err(ApiError::bad_request("Say what the agent should do.").into());
    }

    let model = machine::picked_model(start.model.as_deref())?;
    let (host, why) = match &start.machine_id {
        Some(id) => {
            let machine = fleet.member(id).ok_or_else(not_found)?;
            let host = named_host(fleet, machine, start.agent.as_deref())?;
            let host = if start.assistant {
                for_assistant(vec![host], |host| runs_assistant(fleet, host))?
                    .pop()
                    .ok_or(ApiError::Internal {})?
            } else {
                host
            };
            (host, String::new())
        }
        None => pick(fleet, &start, &prompt, limit).await?,
    };
    let machine = fleet.member(&host.machine_id).ok_or_else(not_found)?;
    let agent = machine::pick_agent(&host, start.agent.as_deref())?;
    let effort = machine::picked_effort(&agent, start.effort)?;
    let fast = start.fast;
    let account = start.account;

    if let Some(account) = &account
        && machine != fleet.id()
        && !newer_than(fleet.status().version(machine).as_deref(), &BEFORE_ACCOUNTS)
    {
        return Err(DesktopError::BadRequest(format!(
            "Update Beep on {} to start on the account {account}.",
            host.machine_name
        )));
    }

    let checkout = machine::checkout_for(
        Makes {
            checkouts: true,
            branches: true,
        },
        &host.machine_name,
        start.checkout,
    )?;
    let cwd = start
        .cwd
        .map(|cwd| cwd.trim().to_owned())
        .filter(|cwd| !cwd.is_empty());
    let first = Request::Start {
        cwd,
        prompt,
        model,
        effort,
        fast,
        checkout,
        images,
        assistant: start.assistant,
        continues: start.continues,
    };
    Ok(Planned {
        machine,
        agent,
        first,
        why,
        account,
    })
}

/// The machine a start names, as a place to run `agent`: as it last said
/// what it offers, on or off now. One that has not said since the app
/// opened is taken to offer the agent named, and its node decides.
fn named_host(fleet: &Fleet, machine: EndpointId, agent: Option<&str>) -> Result<AgentHost> {
    let last = fleet
        .status()
        .last_agent_host(machine, |agent| fleet.harness_off(agent));

    match (last, agent) {
        (Some(host), _) => Ok(host),
        (None, Some(agent)) if !fleet.harness_off(agent) => Ok(AgentHost {
            machine_id: machine.to_string(),
            machine_name: fleet.name_of(machine),
            agents: vec![agent.to_owned()],
            workspace: None,
            home: None,
            capabilities: Vec::new(),
        }),
        (None, _) => Err(ApiError::bad_request(format!(
            "{} offers no coding agent.",
            fleet.name_of(machine)
        ))
        .into()),
    }
}

/// Why a start sent to its machine did not start a session.
#[derive(Debug, thiserror::Error)]
pub(crate) enum StartFailed {
    /// The machine could not be reached: the node never had the start.
    #[error(transparent)]
    Unreached(DesktopError),
    /// The link dropped after the node had the start: the session may exist.
    #[error(transparent)]
    Dropped(DesktopError),
    /// The machine refused it, or the app could not send it.
    #[error(transparent)]
    Refused(DesktopError),
}

impl From<StartFailed> for DesktopError {
    fn from(failed: StartFailed) -> Self {
        match failed {
            StartFailed::Unreached(e) | StartFailed::Dropped(e) | StartFailed::Refused(e) => e,
        }
    }
}

/// Sends a planned start to its machine and waits until the node names the
/// session, however long the agent takes to start.
pub(crate) async fn open(
    fleet: &Fleet,
    planned: Planned,
) -> std::result::Result<StartedAgent, StartFailed> {
    let Planned {
        machine,
        agent,
        first,
        why,
        account,
    } = planned;
    let first = match account {
        Some(account) => Request::StartAs {
            account,
            start: Box::new(first),
        },
        None => first,
    };
    let client = fleet.client(machine).await.map_err(|e| {
        if e.is_transport() {
            StartFailed::Unreached(e)
        } else {
            StartFailed::Refused(e)
        }
    })?;
    let could_not = |e: MeshError| {
        DesktopError::BadRequest(format!("Could not start {}: {e:#}", agent_name(&agent)))
    };
    let opened =
        open_agent_staged(&client, &agent, first)
            .await
            .map_err(|failed| match failed {
                OpenFailed::Unsent(e) if e.is_transport() => StartFailed::Unreached(could_not(e)),
                OpenFailed::Sent(e) if e.is_transport() => StartFailed::Dropped(could_not(e)),
                OpenFailed::Unsent(e) | OpenFailed::Sent(e) => StartFailed::Refused(could_not(e)),
            })?;
    Ok(started(&fleet.host_of(machine), opened.info, why))
}

/// The machine Auto picks for `start` and why.
async fn pick(
    fleet: &Fleet,
    start: &StartAgentSession,
    prompt: &str,
    limit: Duration,
) -> Result<(AgentHost, String)> {
    let candidates = if start.assistant {
        for_assistant(hosts(fleet), |host| runs_assistant(fleet, host))?
    } else {
        hosts(fleet)
    };

    if start.assistant
        && let Some(here) =
            this_computer(&candidates, &fleet.id().to_string(), start.agent.as_deref())
    {
        return Ok((here, "It is this computer.".to_owned()));
    }

    let mut asking = Vec::new();
    let mut last_error = None;
    let clients = futures::future::join_all(candidates.into_iter().map(|host| async move {
        let Some(machine) = fleet.member(&host.machine_id) else {
            return Err(not_found());
        };
        match timeout(limit, fleet.client(machine)).await {
            Ok(Ok(client)) => Ok(AutoHost { client, host }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(DesktopError::TimedOut(host.machine_name)),
        }
    }))
    .await;

    for candidate in clients {
        match candidate {
            Ok(auto) => asking.push(auto),
            Err(e) => last_error = Some(e),
        }
    }

    if asking.is_empty()
        && let Some(e) = last_error
    {
        return Err(e);
    }

    let choice = machine::auto::pick_within(&asking, start, prompt, limit).await?;
    let host = asking
        .into_iter()
        .nth(choice.at)
        .ok_or(ApiError::Internal {})?
        .host;
    Ok((host, choice.why))
}

/// This computer among `hosts`, by its machine id `me`, when it offers
/// `agent`: the assistant starts there before any other machine.
fn this_computer(hosts: &[AgentHost], me: &str, agent: Option<&str>) -> Option<AgentHost> {
    hosts
        .iter()
        .find(|host| {
            host.machine_id == me
                && agent.is_none_or(|agent| host.agents.iter().any(|offered| offered == agent))
        })
        .cloned()
}

/// The hosts whose node runs the home assistant, or, when none does, the
/// update one of them needs.
fn for_assistant(
    hosts: Vec<AgentHost>,
    runs: impl Fn(&AgentHost) -> bool,
) -> Result<Vec<AgentHost>> {
    let (ready, old): (Vec<AgentHost>, Vec<AgentHost>) = hosts.into_iter().partition(runs);

    match (ready.is_empty(), old.first()) {
        (true, Some(old)) => Err(DesktopError::BadRequest(format!(
            "Update Beep on {} to use the assistant.",
            old.machine_name
        ))),
        _ => Ok(ready),
    }
}

/// Whether `host` runs the home assistant: this computer's own node is this
/// build, and another node must be newer than `BEFORE_ASSISTANT`.
pub(crate) fn runs_assistant(fleet: &Fleet, host: &AgentHost) -> bool {
    let Some(machine) = fleet.member(&host.machine_id) else {
        return false;
    };

    machine == fleet.id()
        || newer_than(
            fleet.status().version(machine).as_deref(),
            &BEFORE_ASSISTANT,
        )
}

/// Whether the node of `machine` moves chats: this computer's own node is
/// this build, and another node must be newer than `BEFORE_CHAT_MOVES`.
pub(crate) fn moves_chats(fleet: &Fleet, machine: EndpointId) -> bool {
    machine == fleet.id()
        || newer_than(
            fleet.status().version(machine).as_deref(),
            &BEFORE_CHAT_MOVES,
        )
}

/// Takes over a session started outside Beep; the node refuses one still in
/// a terminal or whose folder is gone.
pub(crate) async fn resume(
    fleet: &Fleet,
    machine: EndpointId,
    resume: ResumeAgentSession,
) -> Result<StartedAgent> {
    let prompt = resume
        .prompt
        .map(|prompt| prompt.trim().to_owned())
        .filter(|prompt| !prompt.is_empty());
    let client = fleet.client(machine).await?;
    let first = Request::Resume {
        session: resume.session,
        prompt,
        model: resume.model,
    };
    let opened = open_agent(&client, &resume.agent, first)
        .await
        .map_err(|e| {
            DesktopError::BadRequest(format!(
                "Could not resume the {} session: {e:#}",
                agent_name(&resume.agent)
            ))
        })?;
    Ok(started(&fleet.host_of(machine), opened.info, String::new()))
}

/// Orders a running session, and gives back its stream, still attached.
/// Only sessions Beep runs take orders. A link that drops on the way says so
/// in plain words.
pub(crate) async fn steer(
    fleet: &Fleet,
    machine: EndpointId,
    steer: SteerAgentSession,
) -> Result<Attached> {
    ordered(fleet, machine, steer).await.map_err(|e| {
        if e.is_transport() {
            DesktopError::Lost(fleet.name_of(machine))
        } else {
            e
        }
    })
}

async fn ordered(fleet: &Fleet, machine: EndpointId, steer: SteerAgentSession) -> Result<Attached> {
    // Refused before the machine is asked.
    let request = machine::node_request(steer.steer)?;
    let images = matches!(request, Request::PromptImages { .. });
    let configure = matches!(request, Request::Configure { .. });
    let drop_steers = matches!(request, Request::DropSteers {});
    let client = fleet.client(machine).await?;
    info(&client, &steer.agent, &steer.session).await?;
    let attach = Request::Attach {
        session: steer.session,
    };
    let mut opened = open_agent(&client, &steer.agent, attach)
        .await
        .map_err(|e| {
            if e.is_transport() {
                DesktopError::Mesh(e)
            } else {
                DesktopError::BadRequest(format!("Could not reach the session: {e:#}"))
            }
        })?;
    // A node from before steering takes the message for after the turn.
    let fallback = machine::queued_instead(&request);
    let sent = match (order(&mut opened, request, ORDER_WAIT).await, fallback) {
        (Err(e), Some(prompt)) if format!("{e:#}").contains("unknown request") => {
            order(&mut opened, prompt, ORDER_WAIT).await
        }
        (sent, _) => sent,
    };
    sent.map(|()| opened).map_err(|e| {
        if e.is_transport() {
            return DesktopError::Unsure(fleet.name_of(machine));
        }

        let error = format!("{e:#}");
        let old = error.contains("unknown request");
        let said = if images && old {
            "beep-node on that machine is too old to send images. Update it, or send text."
        } else if configure && old {
            "beep-node on that machine is too old to switch a running session's model. Update it."
        } else if drop_steers && old {
            "beep-node on that machine is too old to take a message back. Update it."
        } else {
            return DesktopError::BadRequest(error);
        };
        DesktopError::BadRequest(said.into())
    })
}

/// Waits until a session `machine` took a close for has ended, on the stream
/// the close went down: the node acknowledges it before the agent has stopped.
#[tracing::instrument(skip_all, fields(peer = %machine.fmt_short()))]
pub(crate) async fn until_ended(fleet: &Fleet, machine: EndpointId, mut opened: Attached) {
    if let Err(e) = ended(&mut opened, CLOSE_WAIT).await {
        log::warn!("closing a session on {}: {e:#}", fleet.name_of(machine));
    }
}

/// The models `list.agent` can run on a machine; lookup failures remain errors.
pub(crate) async fn models(
    fleet: &Fleet,
    machine: EndpointId,
    list: &ListAgentModels,
) -> Result<AgentModels> {
    let client = fleet.client(machine).await?;
    let host = fleet.agent_host(machine);

    if !host.is_some_and(|host| host.agents.contains(&list.agent)) {
        return Err(ApiError::bad_request(format!(
            "{} does not offer {}.",
            fleet.name_of(machine),
            agent_name(&list.agent)
        ))
        .into());
    }

    let found = sessions::models(&client, &list.agent).await?;
    Ok(machine::agent_models(found))
}

/// Part of a media file on a machine; nothing is kept.
pub(crate) async fn file(
    fleet: &Fleet,
    machine: EndpointId,
    get: &GetAgentFile,
) -> Result<AgentFileChunk> {
    if media_mime(&get.path).is_none() {
        return Err(ApiError::bad_request(
            "Only pictures, videos, audio, 3D models and PDFs can be shown.",
        )
        .into());
    }

    let client = fleet.client(machine).await?;
    chunk(
        sessions::media(&client, &get.path, get.offset).await?,
        fleet,
        machine,
    )
}

/// The chunk a node gave, or why not.
pub(super) fn chunk(read: NodeFile, fleet: &Fleet, machine: EndpointId) -> Result<AgentFileChunk> {
    match read {
        NodeFile::Chunk { size, mime, data } if data.len() <= AGENT_FILE_CHUNK => {
            Ok(AgentFileChunk { size, mime, data })
        }
        NodeFile::Chunk { .. } => {
            Err(ApiError::bad_request("A part of the file is too large.").into())
        }
        NodeFile::Missing => Err(not_found()),
        NodeFile::Refused(message) => Err(ApiError::bad_request(format!(
            "{} could not read the file: {message}",
            fleet.name_of(machine)
        ))
        .into()),
    }
}

/// The picture a session's log shows by `key`. The windows draw it as it
/// comes; there is no small copy to make on this Mac.
pub(crate) async fn image(
    fleet: &Fleet,
    machine: EndpointId,
    get: &GetAgentImage,
) -> Result<AgentImageData> {
    let client = fleet.client(machine).await?;
    let found = sessions::image(&client, &get.session, &get.key)
        .await?
        .ok_or_else(not_found)?;
    if image_key(&found.data) != get.key {
        return Err(not_found());
    }

    Ok(picture(get, found))
}

fn picture(get: &GetAgentImage, found: Picture) -> AgentImageData {
    AgentImageData {
        key: get.key.clone(),
        mime: found.mime,
        data: found.data,
    }
}

/// The picture a session's log shows by `key`, made small on its machine.
/// `None` from a node from before thumbnails, which closes the stream: the
/// caller then reads it whole.
pub(crate) async fn thumb(
    fleet: &Fleet,
    machine: EndpointId,
    get: &GetAgentImage,
) -> Result<Option<AgentImageData>> {
    let client = fleet.client(machine).await?;
    let found = match sessions::thumb(&client, &get.session, &get.key).await {
        Err(MeshError::Link(e)) if e.downcast_ref::<Elapsed>().is_none() => return Ok(None),
        other => other?.ok_or_else(not_found)?,
    };
    Ok(Some(picture(get, found)))
}

/// The custom agents the harnesses define for `folder` on a machine.
pub(crate) async fn custom_agents(
    fleet: &Fleet,
    machine: EndpointId,
    folder: &str,
) -> Result<Vec<CustomAgent>> {
    let client = fleet.client(machine).await?;
    work(
        fleet,
        machine,
        sessions::custom_agents(&client, folder).await?,
    )
}

/// The worktrees of the repository holding `folder` on a machine.
pub(crate) async fn worktrees(
    fleet: &Fleet,
    machine: EndpointId,
    folder: &str,
    session: Option<&SessionId>,
) -> Result<Vec<Worktree>> {
    let client = fleet.client(machine).await?;
    work(
        fleet,
        machine,
        sessions::worktrees(&client, folder, session).await?,
    )
}

/// The commands and skills the running session `session` offers on a
/// machine.
pub(crate) async fn commands(
    fleet: &Fleet,
    machine: EndpointId,
    session: &SessionId,
) -> Result<Vec<AgentCommand>> {
    let client = fleet.client(machine).await?;
    work(fleet, machine, sessions::commands(&client, session).await?)
}

/// A session whose place is asked for: `external` for one started outside Beep.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct PlaceAsk {
    agent: String,
    session: SessionId,
    external: bool,
}

/// Where each session of `asks` works, in order, as its node worked it out
/// from what the agent records; `None` for one the node does not say. Beep's
/// sessions come from one listing, however many are asked.
pub(crate) async fn places(
    fleet: &Fleet,
    machine: EndpointId,
    asks: &[PlaceAsk],
) -> Result<Vec<Option<WorkPlace>>> {
    let client = fleet.client(machine).await?;
    // Lists may still show a session that just ended or is not named yet.
    let listed = if asks.iter().any(|ask| !ask.external) {
        limited(sessions::list_all(&client)).await?
    } else {
        Vec::new()
    };
    let outside = outside_places(&client, asks).await?;

    Ok(asks
        .iter()
        .map(|ask| {
            if ask.external {
                return outside
                    .get(&(ask.agent.as_str(), &ask.session))
                    .cloned()
                    .flatten();
            }

            listed
                .iter()
                .find(|info| info.id == ask.session && info.agent == ask.agent)
                .and_then(|info| info.place.clone())
        })
        .collect())
}

/// Where the sessions of `asks` started outside Beep work, by agent and id:
/// one request for them all, or one each, a few at a time, of a node from
/// before that.
async fn outside_places<'a>(
    client: &PeerClient,
    asks: &'a [PlaceAsk],
) -> Result<HashMap<(&'a str, &'a SessionId), Option<WorkPlace>>> {
    let outside: Vec<&PlaceAsk> = asks.iter().filter(|ask| ask.external).collect();

    if outside.is_empty() {
        return Ok(HashMap::new());
    }

    let mut found = Vec::new();

    for part in outside.chunks(agent::MAX_EXTERNAL_NAMED) {
        let named = part
            .iter()
            .map(|ask| agent::ExternalAsk {
                agent: ask.agent.clone(),
                session: ask.session.clone(),
            })
            .collect();
        let Some(more) = limited(sessions::external_named(client, named)).await? else {
            return outside_one_by_one(client, outside).await;
        };
        found.extend(more);
    }

    Ok(outside
        .iter()
        .map(|ask| {
            let place = found
                .iter()
                .find(|found| found.id == ask.session && found.agent == ask.agent)
                .and_then(|found| found.place.clone());
            ((ask.agent.as_str(), &ask.session), place)
        })
        .collect())
}

/// `outside_places` of a node from before `ExternalNamed`.
async fn outside_one_by_one<'a>(
    client: &PeerClient,
    outside: Vec<&'a PlaceAsk>,
) -> Result<HashMap<(&'a str, &'a SessionId), Option<WorkPlace>>> {
    let asks: Vec<_> = outside
        .into_iter()
        .map(|ask| async move {
            let found = limited(sessions::external(
                client,
                &ask.agent,
                1,
                Some(ask.session.clone()),
            ))
            .await?;
            let place = found.into_iter().next().and_then(|found| found.place);
            Ok(((ask.agent.as_str(), &ask.session), place))
        })
        .collect();
    stream::iter(asks)
        .buffer_unordered(IN_FLIGHT)
        .collect::<Vec<Result<_>>>()
        .await
        .into_iter()
        .collect()
}

/// What a node did in a folder, or its refusal as the user reads it.
pub(super) fn work<T>(fleet: &Fleet, machine: EndpointId, done: NodeWork<T>) -> Result<T> {
    match done {
        NodeWork::Done(done) => Ok(done),
        NodeWork::Refused(message) => {
            Err(ApiError::bad_request(format!("{}: {message}", fleet.name_of(machine))).into())
        }
    }
}

/// What a machine finds of each harness, with the shared settings; `refresh` looks versions up again.
pub(crate) async fn harnesses(
    fleet: &std::sync::Arc<Fleet>,
    machine: EndpointId,
    refresh: bool,
) -> Result<AgentHarnesses> {
    if !refresh {
        let fleet_arc = fleet.clone();
        let report = fleet
            .projections()
            .harnesses
            .read(
                machine,
                PartialEq::eq,
                || {},
                move || async move {
                    let client = fleet_arc.client(machine).await?;
                    sessions::harnesses(&client, false)
                        .await?
                        .ok_or_else(|| too_old(&fleet_arc, machine))
                },
            )
            .await?;
        return Ok(harnesses_view(fleet, machine, report));
    }

    let client = fleet.client(machine).await?;
    let report = sessions::harnesses(&client, true)
        .await?
        .ok_or_else(|| too_old(fleet, machine))?;
    fleet.projections().harnesses.put(machine, report.clone());
    Ok(harnesses_view(fleet, machine, report))
}

/// The harnesses of each machine online, as `agent_harnesses` gives them without a refresh,
/// for the accounts view to push.
pub(crate) async fn harness_views(fleet: &std::sync::Arc<Fleet>) -> Vec<super::view_signal::View> {
    use super::view_signal::View;

    let reads = fleet.online().into_iter().map(|machine| async move {
        let args = serde_json::json!({ "machineId": machine.to_string() });

        match harnesses(fleet, machine, false).await {
            Ok(list) => View::new("agent_harnesses", args, list),
            Err(error) => Some(View::failed(
                "agent_harnesses",
                args,
                &crate::settings::credentials::plain(error).to_string(),
            )),
        }
    });

    futures::future::join_all(reads)
        .await
        .into_iter()
        .flatten()
        .collect()
}

/// How long the models of an agent on a machine hold before they are read again.
const MODELS_FRESH: Duration = Duration::from_secs(300);

/// `agent_models` for each harness each machine offers, for the machines and accounts views to
/// push: the models change only when a harness or an account does. One that the machine has not
/// answered yet is left out and comes with the redraw its answer rings.
pub(crate) async fn model_views(
    app: &tauri::AppHandle,
    fleet: &Arc<Fleet>,
) -> Vec<super::view_signal::View> {
    use super::view_signal::View;

    let reads = hosts(fleet).into_iter().flat_map(|host| {
        host.agents
            .into_iter()
            .map(move |agent| (host.machine_id.clone(), agent))
    });
    let reads = reads.filter_map(|(machine_id, agent)| {
        let machine = fleet.member(&machine_id)?;
        let app = app.clone();
        Some(async move {
            let args = serde_json::json!({ "machineId": machine_id, "agent": agent });
            let notify = move || {
                ring(
                    &app,
                    Change::Topic(beep_proto::node::events::Topic::new("accounts")),
                );
            };

            let request = ListAgentModels {
                machine_id: machine_id.clone(),
                agent: agent.clone(),
            };
            let fleet_arc = fleet.clone();
            let read = fleet
                .projections()
                .models
                .read_fast_with_fresh(
                    (machine, agent.clone()),
                    |_| MODELS_FRESH,
                    PartialEq::eq,
                    notify,
                    move || async move { models(&fleet_arc, machine, &request).await },
                )
                .await?;

            match read {
                Ok(list) => View::new("agent_models", args, Some(list)),
                Err(error) => Some(View::failed("agent_models", args, &error.to_string())),
            }
        })
    });

    futures::future::join_all(reads)
        .await
        .into_iter()
        .flatten()
        .collect()
}

/// Installs or updates `agent` on a machine.
pub(crate) async fn update_harness(
    fleet: &Fleet,
    machine: EndpointId,
    agent: &str,
) -> Result<AgentHarnesses> {
    let client = fleet.client(machine).await?;
    let report = sessions::update_harness(&client, agent)
        .await
        .map_err(|e| {
            DesktopError::BadRequest(format!(
                "Could not update {} on {}: {}",
                agent_name(agent),
                fleet.name_of(machine),
                beep_proto::node::harness::clean_output(&format!("{e:#}"))
            ))
        })?
        .ok_or_else(|| too_old(fleet, machine))?;
    fleet.projections().harnesses.put(machine, report.clone());
    Ok(harnesses_view(fleet, machine, report))
}

/// A harness or its settings changed on `machine`: its models are read again, and the views
/// that list them draw with what the read finds.
pub(crate) fn models_changed(app: &tauri::AppHandle, fleet: &Fleet, machine: EndpointId) {
    fleet.projections().forget_models_on(machine);
    ring(
        app,
        Change::Topic(beep_proto::node::events::Topic::new("accounts")),
    );
}

fn too_old(fleet: &Fleet, machine: EndpointId) -> DesktopError {
    DesktopError::BadRequest(format!(
        "Update beep-node on {} to manage its coding agents here.",
        fleet.name_of(machine)
    ))
}

fn harnesses_view(
    fleet: &Fleet,
    machine: EndpointId,
    report: beep_proto::node::harness::HarnessReport,
) -> AgentHarnesses {
    AgentHarnesses {
        machine_id: machine.to_string(),
        machine_name: fleet.name_of(machine),
        checked_at: (report.checked_at > 0)
            .then(|| UNIX_EPOCH + Duration::from_secs(report.checked_at)),
        harnesses: report
            .harnesses
            .into_iter()
            .map(|info| harness_view(fleet, info))
            .collect(),
    }
}

/// One harness as the page shows it, with the shared settings for it.
fn harness_view(fleet: &Fleet, info: HarnessInfo) -> AgentHarness {
    let known = harness(&info.agent);
    let sign_in = match info.signed_in {
        Some(true) => HarnessSignIn::SignedIn {
            account: info.account,
            plan: info.plan,
        },
        Some(false) => HarnessSignIn::SignedOut {},
        None => HarnessSignIn::Unknown {},
    };
    let config = fleet
        .mesh()
        .and_then(|mesh| mesh.get(&Key::Harness(info.agent.clone())))
        .and_then(|bytes| HarnessConfig::from_bytes(&bytes).ok());
    let settings = config.map_or_else(
        || HarnessSettings {
            enabled: true,
            program: String::new(),
            home: String::new(),
            args: Vec::new(),
            env: Vec::new(),
        },
        |config| HarnessSettings {
            enabled: config.enabled,
            program: config.program.unwrap_or_default(),
            home: config.home.unwrap_or_default(),
            args: config.args,
            // Never the values.
            env: config
                .env
                .into_iter()
                .map(|var| HarnessEnv {
                    name: var.name,
                    value: None,
                })
                .collect(),
        },
    );
    AgentHarness {
        agent: info.agent,
        installed: info.installed,
        runnable: info.runnable,
        version: info.version,
        latest: info.latest,
        program: info.program,
        home_var: known.map(|known| known.home_var.to_owned()),
        home_default: known.map(|known| known.home_default.to_owned()),
        sign_in,
        install: info.install,
        settings,
    }
}

/// Starts Codex's device sign-in on a machine and waits for its code.
pub(crate) async fn codex_login_start(fleet: &Fleet, machine: EndpointId) -> Result<CodexLogin> {
    let client = fleet.client(machine).await?;
    codex_login::start(&client)
        .await
        .map_err(|e| DesktopError::BadRequest(format!("{}: {e}", fleet.name_of(machine))))
}

pub(crate) async fn codex_login_state(
    fleet: &Fleet,
    machine: EndpointId,
    task: u32,
) -> Result<CodexLoginState> {
    let client = fleet.client(machine).await?;
    Ok(codex_login::state(&client, task).await?)
}

pub(crate) async fn codex_login_cancel(
    fleet: &Fleet,
    machine: EndpointId,
    task: u32,
) -> Result<()> {
    let client = fleet.client(machine).await?;
    Ok(codex_login::cancel(&client, task).await?)
}

#[cfg(test)]
#[path = "../tests/fleet_agents.rs"]
mod tests;
