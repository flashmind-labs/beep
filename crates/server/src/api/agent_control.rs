//! Starting and steering coding-agent sessions from the app: the machines
//! that can run one, starting one (on a named machine or by Auto), reading
//! one in full and sending it a message, a stop or a close.

use super::{
    Reply,
    agent_auto::{self, Candidate, Wants},
    agent_sessions::{Host, IN_FLIGHT, NODE_LIMIT, beep_session, external_session, limited},
    harnesses,
    machines::machine_capability,
    own_nodes::{self, linked_keys},
};
use crate::{
    nodes::{Follower, NodeFile, Online, StartOn, agent_name},
    protocol::{
        node::agent::{self, PlanStatus, Request, SessionInfo},
        *,
    },
    router::Cx,
    store::{AgentImageBytes, AgentImageOf},
    tools::machines::command_name,
};
use futures::{StreamExt, stream};
use std::time::{Duration, Instant};
use tokio::time::timeout;

/// The most transcript entries one view reads.
const MAX_ENTRIES: u32 = 200;
/// The agent Auto starts when the request names none, in this order of
/// preference among those the machine offers.
const PREFERRED: [&str; 6] = ["claude", "codex", "gemini", "opencode", "pi", "flsh"];

/// An online machine of the caller's that offers a coding agent.
pub(super) struct Live {
    pub(super) host: AgentHost,
    pub(super) online: Online,
}

impl Live {
    pub(super) fn host(&self) -> Host {
        Host {
            id: self.host.machine_id.clone(),
            name: self.host.machine_name.clone(),
        }
    }

    fn offers(&self, agent: &str) -> bool {
        self.host.agents.iter().any(|offered| offered == agent)
    }

    /// `named`, or the one this machine prefers when the call names none.
    pub(super) fn agent(&self, named: Option<&str>) -> Result<String, ApiError> {
        if let Some(named) = named {
            return if self.offers(named) {
                Ok(named.to_owned())
            } else {
                Err(ApiError::bad_request(format!(
                    "{} does not offer {}.",
                    self.host.machine_name,
                    agent_name(named)
                )))
            };
        }
        PREFERRED
            .iter()
            .find(|agent| self.offers(agent))
            .map(|agent| (*agent).to_owned())
            .or_else(|| self.host.agents.first().cloned())
            .ok_or_else(|| {
                ApiError::bad_request(format!(
                    "{} offers no coding agent.",
                    self.host.machine_name
                ))
            })
    }
}

/// The caller's online machines that offer an agent, in the order the
/// store lists them. A machine whose key was unlinked is left out, and so
/// is an agent the caller turned off on a machine (Coding agents).
pub(super) async fn live_hosts(cx: &Cx) -> Result<Vec<Live>, ApiError> {
    let keys = linked_keys(cx).await?;
    let off = cx.user.off_harnesses().await?;
    Ok(cx
        .user
        .nodes()
        .await?
        .into_iter()
        .filter(|row| keys.contains(&row.endpoint))
        .filter_map(|row| {
            let online = own_nodes::online(cx, &row.endpoint)?;
            let agents: Vec<String> = online
                .offers
                .iter()
                .filter_map(|offer| agent::agent_of(offer))
                .filter(|agent| !harnesses::is_off(&off, &row.endpoint, agent))
                .map(str::to_owned)
                .collect();
            if agents.is_empty() {
                return None;
            }
            let env = row.env.unwrap_or_default();
            let host = AgentHost {
                machine_id: row.endpoint,
                machine_name: row.name,
                agents,
                workspace: online.workspace.clone(),
                home: Some(env.home.clone()).filter(|home| !home.is_empty()),
                capabilities: env
                    .capabilities()
                    .into_iter()
                    .map(machine_capability)
                    .collect(),
            };
            Some(Live { host, online })
        })
        .collect())
}

/// The caller's online machine `machine_id`, when it offers an agent.
pub(super) async fn live_host(cx: &Cx, machine_id: &str) -> Result<Live, ApiError> {
    live_hosts(cx)
        .await?
        .into_iter()
        .find(|live| live.host.machine_id == machine_id)
        .ok_or(ApiError::NotFound {})
}

/// `live_host`, waiting up to `RECONNECT_WAIT` for a machine that dropped
/// its connection lately, as one restarting after an update has.
pub(super) async fn live_host_back(cx: &Cx, machine_id: &str) -> Result<Live, ApiError> {
    match live_host(cx, machine_id).await {
        Err(ApiError::NotFound {}) => {
            if own_nodes::online(cx, machine_id).is_none()
                && cx.app.nodes.back_online(machine_id).await.is_some()
            {
                live_host(cx, machine_id).await
            } else {
                Err(ApiError::NotFound {})
            }
        }
        other => other,
    }
}

/// The caller's own machines, like `ListMachines`.
pub(super) async fn list_agent_hosts(cx: Cx, _: ListAgentHosts) -> Reply<Vec<AgentHost>> {
    Ok(live_hosts(&cx)
        .await?
        .into_iter()
        .map(|live| live.host)
        .collect())
}

/// Takes over the session on its machine; the node refuses one that
/// still runs in a terminal or whose folder is gone.
pub(super) async fn resume_agent_session(
    cx: Cx,
    resume: ResumeAgentSession,
) -> Reply<StartedAgent> {
    let live = live_host(&cx, &resume.machine_id).await?;
    let agent = live.agent(Some(&resume.agent))?;
    let prompt = resume
        .prompt
        .map(|prompt| prompt.trim().to_owned())
        .filter(|prompt| !prompt.is_empty());
    let link = live
        .online
        .take_over(&agent, &resume.session, prompt)
        .await
        .map_err(|e| {
            ApiError::bad_request(format!(
                "Could not resume the {} session: {e:#}",
                agent_name(&agent)
            ))
        })?;
    // It runs as a node session now; nothing follows it from outside.
    cx.app
        .nodes
        .follows
        .unfollow(&cx.app, &live.host.machine_id, &resume.session);
    let info = link.now().info;
    Ok(StartedAgent {
        session: beep_session(&live.host(), info),
        why: String::new(),
    })
}

/// Auto reads what the prompt and folder call for and picks among the
/// machines that have it.
pub(super) async fn start_agent_session(cx: Cx, start: StartAgentSession) -> Reply<StartedAgent> {
    let prompt = start.prompt.trim().to_owned();
    if prompt.is_empty() {
        return Err(ApiError::bad_request("Say what the agent should do."));
    }
    let model = picked_model(start.model.as_deref())?;
    let hosts = live_hosts(&cx).await?;
    if let Some(chat) = start.chat
        && !cx.user.owns_session(chat).await?
    {
        return Err(ApiError::NotFound {});
    }
    let (live, why) = match &start.machine_id {
        Some(id) => {
            let live = hosts
                .into_iter()
                .find(|live| &live.host.machine_id == id)
                .ok_or(ApiError::NotFound {})?;
            (live, String::new())
        }
        None => auto(hosts, &start, &prompt).await?,
    };
    let agent = live.agent(start.agent.as_deref())?;
    let effort = picked_effort(&agent, start.effort)?;
    let checkout = picked_checkout(&live.online, start.checkout)?;
    let cwd = start
        .cwd
        .map(|cwd| cwd.trim().to_owned())
        .filter(|cwd| !cwd.is_empty())
        .or_else(|| live.online.workspace.clone());
    let link = live
        .online
        .start_agent(
            &agent,
            cwd,
            prompt,
            StartOn {
                model,
                effort,
                checkout,
            },
        )
        .await
        .map_err(|e| {
            ApiError::bad_request(format!("Could not start {}: {e:#}", agent_name(&agent)))
        })?;
    let info = link.now().info;
    if let Some(chat) = start.chat {
        live.online.report_to(
            &info.id,
            Follower {
                user: cx.user.clone(),
                chat,
                node: command_name(&live.host.machine_name),
                agent,
            },
        );
    }
    Ok(StartedAgent {
        session: beep_session(&live.host(), info),
        why,
    })
}

/// Where Auto would start `preview` now, without starting it.
pub(super) async fn preview_agent_machine(
    cx: Cx,
    preview: PreviewAgentMachine,
) -> Reply<MachinePick> {
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
    let hosts = live_hosts(&cx).await?;
    let (live, why) = auto(hosts, &start, &prompt).await?;
    Ok(MachinePick {
        machine_id: live.host.machine_id,
        machine_name: live.host.machine_name,
        why,
    })
}

/// The machine Auto picks for `start`, and why.
async fn auto(
    hosts: Vec<Live>,
    start: &StartAgentSession,
    prompt: &str,
) -> Result<(Live, String), ApiError> {
    let mut text = prompt.to_owned();
    if let Some(cwd) = &start.cwd {
        text.push('\n');
        text.push_str(cwd);
    }
    let hints: Vec<MachineCapability> = agent_auto::called_for(&text)
        .into_iter()
        .filter(|hint| !start.needs.contains(hint))
        .collect();
    let offers = |live: &Live| {
        start
            .agent
            .as_deref()
            .is_none_or(|agent| live.offers(agent))
    };
    let folder = start.cwd.as_deref().filter(|cwd| !cwd.trim().is_empty());
    let mut candidates: Vec<Candidate<'_>> = hosts
        .iter()
        .map(|live| Candidate {
            name: &live.host.machine_name,
            offers: offers(live),
            capabilities: &live.host.capabilities,
            workspace: live.online.workspace.is_some(),
            has_folder: true,
            running: 0,
        })
        .collect();
    if let Some(folder) = folder {
        // Only machines that could run it are asked; the futures are made
        // up front, since a closure inside the stream would keep the reply
        // future from being `Send`.
        let asks: Vec<_> = (0..hosts.len())
            .filter(|&at| candidates[at].offers && candidates[at].has(&start.needs))
            .map(|at| {
                probed(
                    at,
                    hosts[at].online.clone(),
                    hosts[at].host.home.clone(),
                    folder,
                )
            })
            .collect();
        let found: Vec<(usize, bool)> = stream::iter(asks)
            .buffer_unordered(IN_FLIGHT)
            .collect()
            .await;
        for (at, there) in found {
            candidates[at].has_folder = there;
        }
    }
    let agent = start.agent.as_deref().map_or("a coding agent", agent_name);
    let wants = Wants {
        agent,
        needs: &start.needs,
        hints: &hints,
        folder,
    };
    let pool =
        agent_auto::pool(&candidates, &wants).map_err(|e| ApiError::bad_request(e.to_string()))?;
    if agent_auto::needs_load(&candidates, &pool) {
        let asks: Vec<_> = pool
            .at
            .iter()
            .map(|&at| counted(at, hosts[at].online.clone()))
            .collect();
        let counts: Vec<(usize, u32)> = stream::iter(asks)
            .buffer_unordered(IN_FLIGHT)
            .collect()
            .await;
        for (at, count) in counts {
            candidates[at].running = count;
        }
    }
    let choice = agent_auto::choose(&candidates, &pool, &wants);
    let live = hosts
        .into_iter()
        .nth(choice.at)
        .ok_or(ApiError::Internal {})?;
    Ok((live, choice.why))
}

/// Whether `folder` is on `online`, asked with `test -d`. When the node
/// cannot be asked, a folder under another system's home root (`/Users`
/// on a Linux machine, `/home` on a Mac) counts as missing and anything
/// else as there, so the start can still try.
async fn probed(at: usize, online: Online, home: Option<String>, folder: &str) -> (usize, bool) {
    let asked = match folder_test(folder) {
        Some(command) => timeout(NODE_LIMIT, online.exec(&command, NODE_LIMIT, &mut |_| {})).await,
        None => return (at, plausible(folder, home.as_deref())),
    };
    let there = match asked {
        Ok(Ok(exited)) if exited.exit_code.is_some() => exited.exit_code == Some(0),
        Ok(Ok(_)) => plausible(folder, home.as_deref()),
        Ok(Err(e)) => {
            log::debug!("folder not checked on {}: {e:#}", online.name);
            plausible(folder, home.as_deref())
        }
        Err(_) => {
            log::debug!("folder check timed out on {}", online.name);
            plausible(folder, home.as_deref())
        }
    };
    (at, there)
}

/// `test -d` for `folder`, with a leading `~` left to the node's shell.
fn folder_test(folder: &str) -> Option<String> {
    let quoted = |path: &str| {
        shlex::try_quote(path)
            .ok()
            .map(|quoted| quoted.into_owned())
    };
    let path = if folder == "~" {
        "\"$HOME\"".to_owned()
    } else if let Some(rest) = folder.strip_prefix("~/") {
        format!("\"$HOME\"/{}", quoted(rest)?)
    } else {
        quoted(folder)?
    };
    Some(format!("test -d {path}"))
}

/// Whether `folder` could be on a machine whose home is `home`.
fn plausible(folder: &str, home: Option<&str>) -> bool {
    let Some(home) = home else {
        return true;
    };
    ["/Users/", "/home/"]
        .iter()
        .all(|root| !folder.starts_with(root) || home.starts_with(root.trim_end_matches('/')))
}

async fn counted(at: usize, online: Online) -> (usize, u32) {
    (at, running(&online).await)
}

/// Sessions that have not ended on `online`; a node that does not answer
/// counts as busy, so Auto goes elsewhere.
async fn running(online: &Online) -> u32 {
    match limited(online.agent_sessions()).await {
        Ok(sessions) => {
            let live = sessions
                .iter()
                .filter(|info| info.state.status != agent::Status::Ended)
                .count();
            u32::try_from(live).unwrap_or(u32::MAX)
        }
        Err(e) => {
            log::debug!("sessions not counted for Auto: {e:#}");
            u32::MAX
        }
    }
}

/// A transcript that cannot be read leaves `entries` empty, so the rest
/// still shows.
pub(super) async fn get_agent_session(cx: Cx, get: GetAgentSession) -> Reply<AgentSessionView> {
    let started = Instant::now();
    let live = live_host(&cx, &get.machine_id).await?;
    let host = live.host();
    let found_host = started.elapsed();
    let listed = limited(live.online.agent_sessions()).await?;
    let listed_at = started.elapsed();
    let mut view = match listed
        .into_iter()
        .find(|info| info.id == get.session && info.agent == get.agent)
    {
        Some(info) => beep_view(&host, info),
        None => {
            // A node that cannot list outside sessions (an older one, or
            // one without the agent) has none to find.
            let outside = limited(live.online.external_sessions(
                &get.agent,
                1,
                Some(get.session.clone()),
            ))
            .await
            .unwrap_or_else(|e| {
                log::debug!("outside sessions not listed: {e:#}");
                Vec::new()
            });
            let found = outside
                .into_iter()
                .find(|session| session.id == get.session)
                .ok_or(ApiError::NotFound {})?;
            let last_message = found.last_message.clone();
            AgentSessionView {
                session: external_session(&host, found),
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
    let read_at = started.elapsed();
    let thoughts = cx.supports(Feature::THOUGHTS);
    match timeout(
        NODE_LIMIT,
        live.online.transcript(&get.session, last, thoughts),
    )
    .await
    {
        Ok(Ok((entries, earlier))) => {
            view.entries = entries_for(entries, thoughts);
            view.earlier = earlier;
        }
        Ok(Err(e)) => log::warn!("transcript not read; session={} error={e:#}", get.session),
        Err(_) => log::warn!("transcript not read in time; session={}", get.session),
    }
    let took = started.elapsed();
    if took >= SLOW_READ {
        log::info!(
            "agent session read slowly; session={} host_ms={} list_ms={} outside_ms={} transcript_ms={}",
            get.session,
            found_host.as_millis(),
            (listed_at - found_host).as_millis(),
            (read_at - listed_at).as_millis(),
            (took - read_at).as_millis(),
        );
    }
    Ok(view)
}

/// A session read slower than this is logged with where the time went.
const SLOW_READ: Duration = Duration::from_millis(500);

fn beep_view(host: &Host, info: SessionInfo) -> AgentSessionView {
    let state = info.state.clone();
    let last_message = info.last_message.clone();
    let turns = info.turns;
    AgentSessionView {
        session: beep_session(host, info),
        plan: state
            .plan
            .into_iter()
            .map(|step| AgentPlanStep {
                content: step.content,
                status: plan_status(step.status),
            })
            .collect(),
        files: state.files_touched,
        last_message,
        error: state.error,
        queued: state.queued,
        turns,
        entries: Vec::new(),
        earlier: 0,
    }
}

/// A status from a newer node reads as pending.
fn plan_status(status: PlanStatus) -> AgentPlanStatus {
    match status {
        PlanStatus::InProgress => AgentPlanStatus::InProgress {},
        PlanStatus::Completed => AgentPlanStatus::Completed {},
        _ => AgentPlanStatus::Pending {},
    }
}

/// The entries an app reads: thoughts only for one that reads them.
fn entries_for(entries: Vec<agent::Entry>, thoughts: bool) -> Vec<AgentEntry> {
    entries
        .into_iter()
        .filter(|entry| thoughts || entry.kind != agent::EntryKind::Thought)
        .map(entry)
        .collect()
}

fn entry(entry: agent::Entry) -> AgentEntry {
    let kind = match entry.kind {
        agent::EntryKind::User => AgentEntryKind::User {},
        agent::EntryKind::Agent => AgentEntryKind::Agent {},
        agent::EntryKind::Tool => AgentEntryKind::Tool {},
        agent::EntryKind::Question => AgentEntryKind::Question {},
        agent::EntryKind::Thought => AgentEntryKind::Thought { ms: entry.ms },
        _ => AgentEntryKind::Other {},
    };
    AgentEntry {
        kind,
        text: entry.text,
    }
}

/// Only sessions Beep runs take orders; one started outside Beep is not
/// found.
pub(super) async fn steer_agent_session(cx: Cx, steer: SteerAgentSession) -> Reply<()> {
    // Refused before the machine is looked up, so an oversized message
    // costs nothing more.
    let request = node_request(steer.steer)?;
    let images = matches!(request, Request::PromptImages { .. });
    let configure = matches!(request, Request::Configure { .. });
    let drop_steers = matches!(request, Request::DropSteers {});
    let live = live_host(&cx, &steer.machine_id).await?;
    let listed = limited(live.online.agent_sessions()).await?;
    if !listed
        .iter()
        .any(|info| info.id == steer.session && info.agent == steer.agent)
    {
        return Err(ApiError::NotFound {});
    }
    let link = live
        .online
        .agent_session(&steer.agent, &steer.session)
        .await
        .map_err(|e| ApiError::bad_request(format!("Could not reach the session: {e:#}")))?;
    // A node from before steering takes the message as one for after the
    // turn, which is the nearest it can do.
    let fallback = queued_instead(&request);
    let sent = match (link.send(request).await, fallback) {
        (Err(e), Some(prompt)) if format!("{e:#}").contains("unknown request") => {
            link.send(prompt).await
        }
        (sent, _) => sent,
    };
    sent.map_err(|e| {
        let error = format!("{e:#}");
        // A node from before images does not know the request.
        if images && error.contains("unknown request") {
            ApiError::bad_request(
                "beep-node on that machine is too old to send images. Update it, or send text.",
            )
        } else if configure && error.contains("unknown request") {
            ApiError::bad_request(
                "beep-node on that machine is too old to switch a running session's model. Update it.",
            )
        } else if drop_steers && error.contains("unknown request") {
            ApiError::bad_request(
                "beep-node on that machine is too old to take a message back. Update it.",
            )
        } else {
            ApiError::bad_request(error)
        }
    })?;
    Ok(())
}

/// The apps read each picture of a session by its key with
/// `GetAgentImage`; there is nothing to list.
pub(super) async fn list_agent_images(_: Cx, _: ListAgentImages) -> Reply<Vec<AgentImage>> {
    Ok(Vec::new())
}

/// The kept copy of the picture, or else the node's, of which a copy is
/// then kept so it shows while the machine is offline.
pub(super) async fn get_agent_image(cx: Cx, get: GetAgentImage) -> Reply<AgentImageData> {
    if !is_image_key(&get.key) {
        return Err(ApiError::bad_request("That is not an image key."));
    }
    match cx.user.agent_image(&get.key).await {
        Ok(Some(kept)) => {
            return Ok(AgentImageData {
                key: get.key,
                mime: kept.mime,
                data: kept.data,
            });
        }
        Ok(None) => {}
        // The node may still have it.
        Err(e) => log::warn!("agent image not read; key={} error={e:#}", get.key),
    }
    let live = live_host(&cx, &get.machine_id).await?;
    let image = live
        .online
        .agent_image(&get.session, &get.key)
        .await
        .map_err(|e| {
            // A node from before images by key closes the stream.
            ApiError::bad_request(format!(
                "Could not read the image from {}: {e:#}. Updating beep-node there may help.",
                live.host.machine_name
            ))
        })?
        .ok_or(ApiError::NotFound {})?;
    if image_key(&image.data) != get.key {
        log::warn!(
            "node gave another image; session={} key={}",
            get.session,
            get.key
        );
        return Err(ApiError::NotFound {});
    }
    let image = AgentImageBytes {
        mime: image.mime,
        data: image.data,
    };
    let of = AgentImageOf {
        machine_id: &get.machine_id,
        session: &get.session,
    };
    if let Err(e) = cx.user.save_agent_image(of, &image).await {
        log::warn!("agent image not kept; key={} error={e:#}", get.key);
    }
    Ok(AgentImageData {
        key: get.key,
        mime: image.mime,
        data: image.data,
    })
}

/// Part of a media file on one of the caller's machines, read from its
/// node on each call; nothing is kept.
pub(super) async fn get_agent_file(cx: Cx, get: GetAgentFile) -> Reply<AgentFileChunk> {
    if media_mime(&get.path).is_none() {
        return Err(ApiError::bad_request(
            "Only pictures, videos, 3D models and PDFs can be shown.",
        ));
    }
    let live = live_host(&cx, &get.machine_id).await?;
    let read = live
        .online
        .agent_file(&get.path, get.offset)
        .await
        .map_err(|e| {
            // A node from before `File` closes the stream.
            ApiError::bad_request(format!(
                "Could not read the file from {}: {e:#}. Updating beep-node there may help.",
                live.host.machine_name
            ))
        })?;
    match read {
        NodeFile::Chunk { size, mime, data } => Ok(AgentFileChunk { size, mime, data }),
        NodeFile::Missing => Err(ApiError::NotFound {}),
        NodeFile::Refused(message) => Err(ApiError::bad_request(format!(
            "{} could not read the file: {message}",
            live.host.machine_name
        ))),
    }
}

/// The models `list.agent` can run on `list.machine_id`, as its node finds
/// them. A node too old to say gives `Fixed`, so the form hides the field.
pub(super) async fn list_agent_models(cx: Cx, list: ListAgentModels) -> Reply<AgentModels> {
    let live = live_host(&cx, &list.machine_id).await?;
    if !live.offers(&list.agent) {
        return Err(ApiError::bad_request(format!(
            "{} does not offer {}.",
            live.host.machine_name,
            agent_name(&list.agent)
        )));
    }
    let found = live.online.agent_models(&list.agent).await.map_err(|e| {
        ApiError::bad_request(format!(
            "Could not list the models of {} on {}: {e:#}",
            agent_name(&list.agent),
            live.host.machine_name
        ))
    })?;
    Ok(found.map_or(
        AgentModels {
            pick: AgentModelPick::Fixed {},
            current: None,
            models: Vec::new(),
        },
        agent_models,
    ))
}

/// What the app sees of the models a node found.
fn agent_models(list: agent::ModelList) -> AgentModels {
    let pick = match list.pick {
        agent::ModelPick::Listed => AgentModelPick::Listed {},
        agent::ModelPick::Named => AgentModelPick::Named {},
        _ => AgentModelPick::Fixed {},
    };
    AgentModels {
        pick,
        current: list.current,
        models: list
            .models
            .into_iter()
            .map(|model| AgentModel {
                id: model.id,
                name: model.name,
                description: model.description,
            })
            .collect(),
    }
}

/// The model a start asks for: `None` for the agent's own, and refused
/// when it cannot be a model's name.
fn picked_model(model: Option<&str>) -> Result<Option<String>, ApiError> {
    let Some(model) = model.map(str::trim).filter(|model| !model.is_empty()) else {
        return Ok(None);
    };
    if !agent::valid_model(model) {
        return Err(ApiError::bad_request("That is not a model name."));
    }
    Ok(Some(model.to_owned()))
}

/// The thinking level a start asks of `agent`: `None` for the agent's
/// own, and refused when the agent cannot be set to it.
fn picked_effort(
    agent: &str,
    effort: Option<AgentEffort>,
) -> Result<Option<agent::Effort>, ApiError> {
    let Some(effort) = effort.map(agent::Effort::from) else {
        return Ok(None);
    };
    if !agent::efforts(agent).contains(&effort) {
        return Err(ApiError::bad_request(format!(
            "{} cannot be set to think at {}.",
            agent_name(agent),
            effort.as_str()
        )));
    }
    Ok(Some(effort))
}

/// The branch or worktree a start asks `node` to make first: refused when
/// its base cannot name a ref, or the node is too old to make it (it would
/// start in the checkout as it is).
fn picked_checkout(
    node: &Online,
    checkout: Option<AgentCheckoutPlan>,
) -> Result<Option<agent::CheckoutPlan>, ApiError> {
    let makes = Makes {
        checkouts: node.checkouts,
        branches: node.branch_checkouts,
    };
    checkout_for(makes, &node.name, checkout)
}

/// Which checkout plans a node makes.
#[derive(Debug, Clone, Copy)]
struct Makes {
    /// A new branch or worktree named from the prompt.
    checkouts: bool,
    /// A worktree on a branch named by the start, new or existing.
    branches: bool,
}

/// `picked_checkout` for a node named `name` that makes what `makes` says.
fn checkout_for(
    makes: Makes,
    name: &str,
    checkout: Option<AgentCheckoutPlan>,
) -> Result<Option<agent::CheckoutPlan>, ApiError> {
    let Some(plan) = checkout.map(agent::CheckoutPlan::from) else {
        return Ok(None);
    };
    if let Some(base) = plan.base()
        && !agent::valid_ref(base)
    {
        return Err(ApiError::bad_request(format!(
            "{base:?} is not a branch or commit name."
        )));
    }
    if let Some(branch) = plan.branch() {
        if !agent::valid_branch(branch) {
            return Err(ApiError::bad_request(format!(
                "{branch:?} is not a branch name git takes."
            )));
        }
        if !makes.branches {
            return Err(ApiError::bad_request(format!(
                "beep-node on {name} is too old to start on a named branch. Update it, or start on a new worktree."
            )));
        }
    }
    if !makes.checkouts {
        return Err(ApiError::bad_request(format!(
            "beep-node on {name} is too old to make a branch or worktree. Update it, or start in the checkout as it is."
        )));
    }
    Ok(Some(plan))
}

/// Whether `key` is what `image_key` makes: 16 lowercase hex digits.
fn is_image_key(key: &str) -> bool {
    key.len() == 16 && key.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The prompt a node from before `Request::Steer` gets for one: the same
/// message, sent after the running turn.
fn queued_instead(request: &Request) -> Option<Request> {
    let Request::Steer { text, images } = request else {
        return None;
    };
    Some(if images.is_empty() {
        Request::Prompt { text: text.clone() }
    } else {
        Request::PromptImages {
            text: text.clone(),
            images: images.clone(),
        }
    })
}

/// What the node is asked for `steer`; an empty message or images over the
/// limits are refused.
fn node_request(steer: AgentSteer) -> Result<Request, ApiError> {
    Ok(match steer {
        AgentSteer::Prompt { text } if text.trim().is_empty() => {
            return Err(ApiError::bad_request("The message is empty."));
        }
        AgentSteer::Prompt { text } => Request::Prompt { text },
        AgentSteer::PromptImages { text, images } if images.is_empty() => {
            return node_request(AgentSteer::Prompt { text });
        }
        AgentSteer::PromptImages { text, images } => {
            check_agent_images(&images)?;
            Request::PromptImages { text, images }
        }
        AgentSteer::Steer { text, images } if text.trim().is_empty() && images.is_empty() => {
            return Err(ApiError::bad_request("The message is empty."));
        }
        AgentSteer::Steer { text, images } => {
            check_agent_images(&images)?;
            Request::Steer { text, images }
        }
        AgentSteer::DropSteers {} => Request::DropSteers {},
        AgentSteer::Cancel {} => Request::Cancel {},
        AgentSteer::Close {} => Request::Close {},
        AgentSteer::Configure {
            model: None,
            effort: None,
        } => return Err(ApiError::bad_request("Nothing to switch.")),
        AgentSteer::Configure { model, effort } => Request::Configure {
            model: model
                .as_ref()
                .map(|m| m.as_str().trim())
                .filter(|m| !m.is_empty())
                .map(ModelId::from),
            effort: effort.map(Into::into),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_app_that_reads_thoughts_gets_them() {
        let thought = agent::Entry {
            kind: agent::EntryKind::Thought,
            text: "hmm".into(),
            ms: Some(4000),
        };
        let entries = vec![
            agent::Entry::new(agent::EntryKind::User, "go".into()),
            thought,
            agent::Entry::new(agent::EntryKind::Agent, "done".into()),
        ];
        let kinds = |thoughts| -> Vec<AgentEntryKind> {
            entries_for(entries.clone(), thoughts)
                .into_iter()
                .map(|entry| entry.kind)
                .collect()
        };
        assert_eq!(
            kinds(true),
            [
                AgentEntryKind::User {},
                AgentEntryKind::Thought { ms: Some(4000) },
                AgentEntryKind::Agent {},
            ]
        );
        assert_eq!(
            kinds(false),
            [AgentEntryKind::User {}, AgentEntryKind::Agent {}]
        );
    }

    #[test]
    fn a_checkout_plan_needs_a_plain_base_and_a_node_that_makes_it() {
        let worktree = |base: Option<&str>| {
            Some(AgentCheckoutPlan::NewWorktree {
                base: base.map(str::to_owned),
            })
        };
        let old = Makes {
            checkouts: false,
            branches: false,
        };
        let new = Makes {
            checkouts: true,
            branches: true,
        };
        assert_eq!(checkout_for(old, "old-box", None).unwrap(), None);
        assert_eq!(
            checkout_for(new, "box", worktree(Some("main"))).unwrap(),
            Some(agent::CheckoutPlan::NewWorktree {
                base: Some("main".into())
            })
        );
        let ApiError::BadRequest { message } =
            checkout_for(old, "old-box", worktree(None)).unwrap_err()
        else {
            panic!("a bad request");
        };
        assert!(message.contains("old-box is too old"), "{message}");
        assert!(matches!(
            checkout_for(new, "box", worktree(Some("--orphan"))),
            Err(ApiError::BadRequest { .. })
        ));
    }

    #[test]
    fn a_named_branch_needs_a_branch_name_and_a_node_that_makes_it() {
        let named = |branch: &str| {
            Some(AgentCheckoutPlan::NamedWorktree {
                base: Some("main".into()),
                branch: branch.into(),
            })
        };
        let continued = Some(AgentCheckoutPlan::BranchWorktree {
            branch: "login-copy".into(),
        });
        let new = Makes {
            checkouts: true,
            branches: true,
        };
        let plain = Makes {
            checkouts: true,
            branches: false,
        };
        assert_eq!(
            checkout_for(new, "box", named("issue-52-crash")).unwrap(),
            Some(agent::CheckoutPlan::NamedWorktree {
                base: Some("main".into()),
                branch: "issue-52-crash".into(),
            })
        );
        assert_eq!(
            checkout_for(new, "box", continued.clone()).unwrap(),
            Some(agent::CheckoutPlan::BranchWorktree {
                branch: "login-copy".into(),
            })
        );
        for bad in [
            "", "-b", "a..b", "HEAD", "x~1", "a/.b", "a.lock", "a//b", "a b",
        ] {
            assert!(
                matches!(
                    checkout_for(new, "box", named(bad)),
                    Err(ApiError::BadRequest { .. })
                ),
                "{bad:?} is refused"
            );
        }
        let Err(ApiError::BadRequest { message }) = checkout_for(plain, "old-box", continued)
        else {
            panic!("a bad request");
        };
        assert!(message.contains("old-box is too old"), "{message}");
    }

    fn png(name: &str, size: usize) -> Upload {
        let mut data = b"\x89PNG\r\n\x1a\n".to_vec();
        data.resize(size, 0);
        Upload {
            name: name.into(),
            mime: "image/png".into(),
            data,
        }
    }

    fn steer_images(images: Vec<Upload>) -> Result<Request, ApiError> {
        node_request(AgentSteer::PromptImages {
            text: "Look at this".into(),
            images,
        })
    }

    #[test]
    fn images_within_the_limits_go_to_the_node() {
        let five = (0..MAX_AGENT_IMAGES)
            .map(|i| png(&format!("{i}.png"), 1024))
            .collect();
        let Ok(Request::PromptImages { text, images }) = steer_images(five) else {
            panic!("five small images go");
        };
        assert_eq!(text, "Look at this");
        assert_eq!(images.len(), MAX_AGENT_IMAGES);
        assert_eq!(
            steer_images(vec![png("max.png", MAX_AGENT_IMAGE)]).map(|_| ()),
            Ok(())
        );
    }

    #[test]
    fn too_many_images_are_refused() {
        let six = (0..=MAX_AGENT_IMAGES)
            .map(|i| png(&format!("{i}.png"), 1024))
            .collect();
        let error = steer_images(six).unwrap_err().to_string();
        assert!(error.contains("At most 5 images"), "{error}");
    }

    #[test]
    fn an_image_over_the_size_limit_is_refused() {
        let error = steer_images(vec![png("huge.png", MAX_AGENT_IMAGE + 1)])
            .unwrap_err()
            .to_string();
        assert!(error.contains("huge.png is over 5 MB"), "{error}");
    }

    #[test]
    fn a_message_without_images_goes_as_text() {
        assert_eq!(
            steer_images(Vec::new()),
            Ok(Request::Prompt {
                text: "Look at this".into()
            })
        );
        let empty = node_request(AgentSteer::PromptImages {
            text: " ".into(),
            images: Vec::new(),
        });
        assert!(empty.is_err(), "nothing to send");
    }

    #[test]
    fn a_steer_goes_to_the_node_and_falls_back_to_a_prompt() {
        let steer = node_request(AgentSteer::Steer {
            text: "Use the fixture instead".into(),
            images: vec![png("shot.png", 1024)],
        })
        .unwrap();
        assert!(matches!(&steer, Request::Steer { images, .. } if images.len() == 1));
        assert!(matches!(
            queued_instead(&steer),
            Some(Request::PromptImages { text, images }) if text == "Use the fixture instead" && images.len() == 1
        ));
        let words = node_request(AgentSteer::Steer {
            text: "stop editing tests".into(),
            images: Vec::new(),
        })
        .unwrap();
        assert_eq!(
            queued_instead(&words),
            Some(Request::Prompt {
                text: "stop editing tests".into()
            })
        );
        assert_eq!(queued_instead(&Request::Cancel {}), None);
        let empty = node_request(AgentSteer::Steer {
            text: " ".into(),
            images: Vec::new(),
        });
        assert!(empty.is_err(), "nothing to send");
    }

    #[test]
    fn only_image_keys_are_asked_for() {
        assert!(is_image_key(&image_key(b"\x89PNG")));
        assert!(!is_image_key("shot.png"));
        assert!(!is_image_key("0123456789ABCDEF"));
        assert!(!is_image_key("0123456789abcde"));
    }

    #[test]
    fn folder_tests_quote_the_path_and_keep_home() {
        assert_eq!(
            folder_test("/srv/project").as_deref(),
            Some("test -d /srv/project")
        );
        assert_eq!(
            folder_test("/srv/my project").as_deref(),
            Some("test -d '/srv/my project'")
        );
        assert_eq!(
            folder_test("~/code/app").as_deref(),
            Some("test -d \"$HOME\"/code/app")
        );
        assert_eq!(folder_test("~").as_deref(), Some("test -d \"$HOME\""));
    }

    #[test]
    fn an_unchecked_folder_under_another_systems_home_is_missing() {
        assert!(!plausible("/Users/sam/app", Some("/home/sam")));
        assert!(!plausible("/home/sam/app", Some("/Users/sam")));
        assert!(plausible("/Users/sam/app", Some("/Users/sam")));
        assert!(plausible("/srv/project", Some("/home/sam")));
        assert!(plausible("/Users/sam/app", None));
    }
}
