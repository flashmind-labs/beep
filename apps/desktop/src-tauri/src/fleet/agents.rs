//! Coding agents on the machines of the mesh, asked directly: the sessions,
//! starting, steering and answering them, their pictures and files, and
//! the harnesses and sign-ins of each machine. What each returns is what
//! the server returned for the same request, so the windows are the same.

use super::Fleet;
use crate::error::{DesktopError, Result};
use beep_mesh::{
    Key, MeshError, PeerClient,
    machine::{
        self, Host, Makes, StartOn,
        auto::AutoHost,
        codex_login, listing, open_agent, order,
        sessions::{self, NodeFile, NodeWork},
    },
};
use beep_proto::{
    AGENT_FILE_CHUNK, AgentFileChunk, AgentHarness, AgentHarnesses, AgentHost, AgentImageData,
    AgentModels, AgentSessionModes, AgentSessionUsage, AgentSessionView, ApiError, CodexLogin,
    CodexLoginState, CustomAgent, GetAgentFile, GetAgentImage, GetAgentSession,
    GetAgentSessionModes, GetAgentSessionUsage, HarnessEnv, HarnessSettings, HarnessSignIn,
    ListAgentModels, MachinePick, MachineSession, PreviewAgentMachine, ResumeAgentSession,
    StartAgentSession, StartedAgent, SteerAgentSession, image_key, media_mime,
    node::{
        agent::{self, Request},
        harness::{HarnessConfig, HarnessInfo, harness},
        mesh_state::StateValue,
    },
};
use futures::{StreamExt, stream};
use iroh::EndpointId;
use std::{
    future::Future,
    time::{Duration, UNIX_EPOCH},
};
use tokio::time::timeout;

/// The most transcript entries one view reads.
const MAX_ENTRIES: u32 = 200;
/// Machines asked at once.
const IN_FLIGHT: usize = 4;
/// How long an order to a session may take.
const ORDER_WAIT: Duration = Duration::from_secs(10);

fn not_found() -> DesktopError {
    DesktopError::Api(ApiError::NotFound {})
}

/// `call`, given up after `listing::NODE_LIMIT`.
async fn limited<T>(call: impl Future<Output = std::result::Result<T, MeshError>>) -> Result<T> {
    match timeout(listing::NODE_LIMIT, call).await {
        Ok(done) => Ok(done?),
        Err(_) => Err(DesktopError::BadRequest(
            "the machine did not answer in time".into(),
        )),
    }
}

impl Fleet {
    pub(super) fn host_of(&self, machine: EndpointId) -> Host {
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
        |client, host| async move { listing::beep_sessions(&client, &host).await },
    )
    .await
}

/// Beep's sessions on the other online machines of the mesh.
pub(crate) async fn peer_sessions(fleet: &Fleet) -> Vec<MachineSession> {
    let me = fleet.id();
    let peers = fleet.online().into_iter().filter(|m| *m != me).collect();
    each(fleet, peers, MACHINE_LIMIT, |client, host| async move {
        listing::beep_sessions(&client, &host).await
    })
    .await
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
    let listed = limited(sessions::list(&client)).await?;
    let mut view = match listed
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
                session: machine::external_session(&host, found),
                plan: Vec::new(),
                files: Vec::new(),
                last_message,
                error: None,
                queued: 0,
                turns: 0,
                entries: Vec::new(),
                earlier: 0,
            }
        }
    };
    let last = get.last.clamp(1, MAX_ENTRIES);
    match limited(sessions::transcript(&client, &get.session, last, true)).await {
        Ok((entries, earlier)) => {
            view.entries = machine::entries_for(entries, true);
            view.earlier = earlier;
        }
        Err(e) => log::warn!("transcript not read; session={} error={e:#}", get.session),
    }
    Ok(view)
}

/// The session `session` of `agent` as the node lists it now.
async fn info(client: &PeerClient, agent: &str, session: &str) -> Result<agent::SessionInfo> {
    limited(sessions::list(client))
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

/// A session on `machine` named by the node, as a start gives it back.
fn started(host: &Host, info: agent::SessionInfo, why: String) -> StartedAgent {
    StartedAgent {
        session: machine::beep_session(host, info),
        why,
    }
}

/// Starts a session; with no machine named, Auto picks among the mesh's.
pub(crate) async fn start(fleet: &Fleet, start: StartAgentSession) -> Result<StartedAgent> {
    let prompt = start.prompt.trim().to_owned();
    if prompt.is_empty() {
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
    let on = StartOn {
        model,
        effort,
        checkout,
    };
    let first = Request::Start {
        cwd,
        prompt,
        model: on.model,
        effort: on.effort,
        checkout: on.checkout,
    };
    let client = fleet.client(machine).await?;
    let opened = open_agent(&client, &agent, first).await.map_err(|e| {
        DesktopError::BadRequest(format!(
            "Could not start {}: {e:#}",
            machine::agent_name(&agent)
        ))
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

/// Where Auto would start `preview` now, without starting it.
pub(crate) async fn preview_machine(
    fleet: &Fleet,
    preview: PreviewAgentMachine,
) -> Result<MachinePick> {
    let PreviewAgentMachine {
        agent,
        cwd,
        prompt,
        needs,
    } = preview;
    let prompt = prompt.trim().to_owned();
    let start = StartAgentSession {
        machine_id: None,
        agent,
        cwd,
        prompt: prompt.clone(),
        needs,
        chat: None,
        model: None,
        effort: None,
        checkout: None,
    };
    let (host, why) = pick(fleet, &start, &prompt).await?;
    Ok(MachinePick {
        machine_id: host.machine_id,
        machine_name: host.machine_name,
        why,
    })
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
                machine::agent_name(&resume.agent)
            ))
        })?;
    Ok(started(&fleet.host_of(machine), opened.info, String::new()))
}

/// Orders a running session. Only sessions Beep runs take orders.
pub(crate) async fn steer(
    fleet: &Fleet,
    machine: EndpointId,
    steer: SteerAgentSession,
) -> Result<()> {
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
        .map_err(|e| DesktopError::BadRequest(format!("Could not reach the session: {e:#}")))?;
    // A node from before steering takes the message for after the turn.
    let fallback = machine::queued_instead(&request);
    let sent = match (order(&mut opened, request, ORDER_WAIT).await, fallback) {
        (Err(e), Some(prompt)) if format!("{e:#}").contains("unknown request") => {
            order(&mut opened, prompt, ORDER_WAIT).await
        }
        (sent, _) => sent,
    };
    sent.map_err(|e| {
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

/// The models `list.agent` can run on a machine; a node too old to say gives `Fixed`.
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
            machine::agent_name(&list.agent)
        ))
        .into());
    }
    let found = sessions::models(&client, &list.agent).await?;
    Ok(found.map_or(
        AgentModels {
            pick: beep_proto::AgentModelPick::Fixed {},
            current: None,
            models: Vec::new(),
        },
        machine::agent_models,
    ))
}

/// Part of a media file on a machine; nothing is kept.
pub(crate) async fn file(
    fleet: &Fleet,
    machine: EndpointId,
    get: &GetAgentFile,
) -> Result<AgentFileChunk> {
    if media_mime(&get.path).is_none() {
        return Err(ApiError::bad_request(
            "Only pictures, videos, 3D models and PDFs can be shown.",
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
    Ok(AgentImageData {
        key: get.key.clone(),
        mime: found.mime,
        data: found.data,
    })
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
                machine::agent_name(agent),
                fleet.name_of(machine),
                beep_proto::node::harness::clean_output(&format!("{e:#}"))
            ))
        })?
        .ok_or_else(|| too_old(fleet, machine))?;
    Ok(harnesses_view(fleet, machine, report))
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
