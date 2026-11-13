//! Coding agents on the machines of the mesh, asked directly: the sessions,
//! starting, steering and answering them, their pictures and files, and
//! the harnesses and sign-ins of each machine.

use super::Fleet;
use crate::error::{DesktopError, Result};
use beep_mesh::{
    Key, MeshError, PeerClient,
    machine::{
        self, Host, Makes, Opened as Attached,
        auto::AutoHost,
        codex_login, ended,
        listing::{self, Linked},
        open_agent, order,
        sessions::{self, NodeFile, NodeImage, NodeWork},
    },
};
use beep_proto::{
    AGENT_FILE_CHUNK, AgentFileChunk, AgentHarness, AgentHarnesses, AgentHost, AgentImageData,
    AgentModels, AgentSessionModes, AgentSessionTasks, AgentSessionUsage, AgentSessionView,
    ApiError, CodexLogin, CodexLoginState, CustomAgent, GetAgentFile, GetAgentImage,
    GetAgentSession, GetAgentSessionModes, GetAgentSessionTasks, GetAgentSessionUsage, HarnessEnv,
    HarnessSettings, HarnessSignIn, ListAgentModels, MachineSession, ResumeAgentSession,
    StartAgentSession, StartedAgent, SteerAgentSession, Upload, agent_name, image_key, media_mime,
    node::{
        agent::{self, AgentCommand, Request, WorkPlace, Worktree},
        harness::{HarnessConfig, HarnessInfo, harness},
        mesh_state::StateValue,
    },
};
use futures::{StreamExt, stream};
use iroh::EndpointId;
use serde::Deserialize;
use std::{
    collections::HashMap,
    future::Future,
    time::{Duration, UNIX_EPOCH},
};
use tokio::time::{error::Elapsed, timeout};

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

/// What each online machine of `machines` gives from `ask`, concurrently
/// and a few at a time. One that is slow or off is left out after `limit`.
async fn each<T, F, Fut>(
    fleet: &Fleet,
    machines: Vec<EndpointId>,
    limit: Duration,
    ask: F,
) -> Vec<T>
where
    F: Fn(PeerClient, Host) -> Fut,
    Fut: Future<Output = Vec<T>>,
{
    let ask = &ask;
    let asks: Vec<_> = machines
        .into_iter()
        .map(|machine| async move {
            let one = async {
                match fleet.client(machine).await {
                    Ok(client) => ask(client, fleet.host_of(machine)).await,
                    Err(_) => Vec::new(),
                }
            };
            timeout(limit, one).await.unwrap_or_default()
        })
        .collect();
    stream::iter(asks)
        .buffer_unordered(IN_FLIGHT)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .flatten()
        .collect()
}

/// Beep's sessions on this Mac, from the node in this process.
pub(crate) async fn local_sessions(fleet: &Fleet) -> Vec<MachineSession> {
    each(
        fleet,
        vec![fleet.id()],
        MACHINE_LIMIT,
        |client, host| async move { listed(fleet, &client, &host).await },
    )
    .await
}

/// Beep's sessions on the other machines of the mesh, and the machines that
/// may have some but did not say: online ones that did not answer in time,
/// and those still being connected to.
pub(crate) struct PeerSessions {
    pub(crate) sessions: Vec<MachineSession>,
    pub(crate) unanswered: Vec<EndpointId>,
    pub(crate) connecting: Vec<EndpointId>,
}

/// Beep's sessions on the other online machines of the mesh, a few machines
/// at once.
pub(crate) async fn peer_sessions(fleet: &Fleet) -> PeerSessions {
    let me = fleet.id();
    let asks: Vec<_> = fleet
        .online()
        .into_iter()
        .filter(|machine| *machine != me)
        .map(|machine| async move {
            let listed = timeout(MACHINE_LIMIT, sessions_of(fleet, machine)).await;
            (machine, listed.ok().flatten())
        })
        .collect();
    let answers: Vec<_> = stream::iter(asks)
        .buffer_unordered(IN_FLIGHT)
        .collect()
        .await;
    let mut found = PeerSessions {
        sessions: Vec::new(),
        unanswered: Vec::new(),
        connecting: fleet.status().connecting(),
    };

    for (machine, listed) in answers {
        match listed {
            Some(sessions) => found.sessions.extend(sessions),
            None => found.unanswered.push(machine),
        }
    }

    found
}

/// Beep's sessions on `machine` alone, for its change to replace only its
/// rows; `None` when it could not be listed, so the rows held stay.
pub(crate) async fn sessions_of(fleet: &Fleet, machine: EndpointId) -> Option<Vec<MachineSession>> {
    let client = fleet.client(machine).await.ok()?;
    let host = fleet.host_of(machine);
    let linked = timeout(
        MACHINE_LIMIT,
        listing::listed_sessions(&client, &host, true),
    )
    .await
    .ok()??;
    Some(noted(fleet, &host, linked))
}

/// Beep's sessions on one machine, noting which an agent started. Its
/// children are only noted: they open from the parent agent panel.
async fn listed(fleet: &Fleet, client: &PeerClient, host: &Host) -> Vec<MachineSession> {
    let linked = listing::linked_sessions(client, host, true).await;
    noted(fleet, host, linked)
}

/// Top-level sessions, after keeping children for the parent agent panel.
fn noted(fleet: &Fleet, host: &Host, linked: Vec<Linked>) -> Vec<MachineSession> {
    fleet.parents.note(&host.id, &linked);
    linked
        .into_iter()
        .filter(|linked| linked.parent.is_none() && !linked.reviewer)
        .map(|linked| linked.session)
        .collect()
}

/// The sessions started outside Beep that run now, on every online machine.
pub(crate) async fn scanned_sessions(fleet: &Fleet) -> Vec<MachineSession> {
    each(
        fleet,
        fleet.online(),
        SCAN_LIMIT,
        |client, host| async move { listing::outside_sessions(&client, &host).await },
    )
    .await
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
async fn info(client: &PeerClient, agent: &str, session: &str) -> Result<agent::SessionInfo> {
    limited(sessions::list_all(client))
        .await?
        .into_iter()
        .find(|info| info.id == session && info.agent == agent)
        .ok_or_else(not_found)
}

pub(crate) async fn usage(
    fleet: &Fleet,
    machine: EndpointId,
    get: &GetAgentSessionUsage,
) -> Result<AgentSessionUsage> {
    let client = fleet.client(machine).await?;
    let info = info(&client, &get.agent, &get.session).await?;
    Ok(AgentSessionUsage {
        context: info.context,
        tokens: info.tokens,
        cost_usd: info.cost_usd,
        credits: info.credits,
    })
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
    let prompt = start.prompt.trim().to_owned();

    if prompt.is_empty() && images.is_empty() {
        return Err(ApiError::bad_request("Say what the agent should do.").into());
    }

    let model = machine::picked_model(start.model.as_deref())?;
    let (host, why) = match &start.machine_id {
        Some(id) => {
            let machine = fleet.member(id).ok_or_else(not_found)?;
            let host = fleet.agent_host(machine).ok_or_else(|| {
                ApiError::bad_request(format!(
                    "{} offers no coding agent.",
                    fleet.name_of(machine)
                ))
            })?;
            (host, String::new())
        }
        None => pick(fleet, &start, &prompt).await?,
    };
    let machine = fleet.member(&host.machine_id).ok_or_else(not_found)?;
    let agent = machine::pick_agent(&host, start.agent.as_deref())?;
    let effort = machine::picked_effort(&agent, start.effort)?;
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
        checkout,
        images,
    };
    let client = fleet.client(machine).await?;
    let opened = open_agent(&client, &agent, first).await.map_err(|e| {
        DesktopError::BadRequest(format!("Could not start {}: {e:#}", agent_name(&agent)))
    })?;
    Ok(started(&fleet.host_of(machine), opened.info, why))
}

/// The machine Auto picks for `start` and why.
async fn pick(
    fleet: &Fleet,
    start: &StartAgentSession,
    prompt: &str,
) -> Result<(AgentHost, String)> {
    let mut asking = Vec::new();

    for host in hosts(fleet) {
        let machine = fleet.member(&host.machine_id).ok_or_else(not_found)?;
        asking.push(AutoHost {
            client: fleet.client(machine).await?,
            host,
        });
    }

    let choice = machine::auto::pick(&asking, start, prompt).await?;
    let host = asking
        .into_iter()
        .nth(choice.at)
        .ok_or(ApiError::Internal {})?
        .host;
    Ok((host, choice.why))
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

/// Deletes the ended session `session` of `agent` from a machine; the node
/// refuses one that runs, saying why.
pub(crate) async fn delete_session(
    fleet: &Fleet,
    machine: EndpointId,
    agent: &str,
    session: &str,
) -> Result<()> {
    let client = fleet.client(machine).await?;
    let done = sessions::delete_session(&client, agent, session)
        .await
        .map_err(|e| {
            DesktopError::BadRequest(format!(
                "Could not delete the session on {}: {e:#}",
                fleet.name_of(machine)
            ))
        })?;
    work(fleet, machine, done)
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

fn picture(get: &GetAgentImage, found: NodeImage) -> AgentImageData {
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
) -> Result<Vec<Worktree>> {
    let client = fleet.client(machine).await?;
    work(fleet, machine, sessions::worktrees(&client, folder).await?)
}

/// The commands and skills the running session `session` offers on a
/// machine.
pub(crate) async fn commands(
    fleet: &Fleet,
    machine: EndpointId,
    session: &str,
) -> Result<Vec<AgentCommand>> {
    let client = fleet.client(machine).await?;
    work(fleet, machine, sessions::commands(&client, session).await?)
}

/// A session whose place is asked for: `external` for one started outside Beep.
#[derive(Debug, Deserialize)]
pub struct PlaceAsk {
    agent: String,
    session: String,
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
                    .get(&(ask.agent.as_str(), ask.session.as_str()))
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
) -> Result<HashMap<(&'a str, &'a str), Option<WorkPlace>>> {
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
            ((ask.agent.as_str(), ask.session.as_str()), place)
        })
        .collect())
}

/// `outside_places` of a node from before `ExternalNamed`.
async fn outside_one_by_one<'a>(
    client: &PeerClient,
    outside: Vec<&'a PlaceAsk>,
) -> Result<HashMap<(&'a str, &'a str), Option<WorkPlace>>> {
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
            Ok(((ask.agent.as_str(), ask.session.as_str()), place))
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
    fleet: &Fleet,
    machine: EndpointId,
    refresh: bool,
) -> Result<AgentHarnesses> {
    let client = fleet.client(machine).await?;
    let report = sessions::harnesses(&client, refresh)
        .await?
        .ok_or_else(|| too_old(fleet, machine))?;
    Ok(harnesses_view(fleet, machine, report))
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
    Ok(harnesses_view(fleet, machine, report))
}

/// Makes a machine install the ACP adapters its agents lack now.
pub(crate) async fn install_adapters(fleet: &Fleet, machine: EndpointId) -> Result<()> {
    let client = fleet.client(machine).await?;
    if sessions::install_adapters(&client).await? {
        Ok(())
    } else {
        Err(too_old(fleet, machine))
    }
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
