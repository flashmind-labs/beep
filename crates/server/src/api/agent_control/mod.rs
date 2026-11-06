//! Starting, reading and steering coding-agent sessions from the app.

mod auto;
mod pictures;
mod steer;
mod view;
use auto::auto;
pub(super) use pictures::{get_agent_image, get_agent_thumb};
pub(super) use steer::steer_agent_session;
pub(super) use view::{
    get_agent_session, get_agent_session_modes, get_agent_session_tasks, get_agent_session_usage,
};

use super::{
    Reply,
    agent_sessions::{Host, beep_session},
    harnesses,
    machines::machine_capability,
    own_nodes::{self, linked_keys},
};
use crate::{
    api::router::Cx,
    nodes::{Follower, NodeFile, Online, StartOn, agent_name},
    protocol::{
        node::agent::{self},
        *,
    },
    store::SessionOn,
    tools::machines::command_name,
};
use beep_mesh::machine::{Makes, checkout_for, pick_agent, picked_effort, picked_model};

/// The most transcript entries one view reads.
const MAX_ENTRIES: u32 = 200;

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
        pick_agent(&self.host, named)
    }
}

/// The caller's linked online machines that offer an agent, without agents turned off there.
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

/// Takes over the session; the node refuses one still in a terminal or whose folder is gone.
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
    started_here(&cx, &live, &agent, &info.id).await;
    Ok(StartedAgent {
        session: beep_session(&live.host(), info),
        why: String::new(),
    })
}

/// Auto picks among the machines that have what the prompt and folder call for.
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
    started_here(&cx, &live, &agent, &info.id).await;
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

/// Remembers that the caller's device started `session`, so its alerts go to that device alone.
async fn started_here(cx: &Cx, live: &Live, agent: &str, session: &str) {
    let on = SessionOn {
        machine: &live.online.connection.remote_id(),
        agent,
        session,
    };
    let device = cx.key;
    if let Err(e) = cx.user.record_agent_start(on, &device).await {
        log::warn!("agent start not saved; session={session} error={e:#}");
    }
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

/// The apps read each picture by key with `GetAgentImage`; there is nothing to list.
pub(super) async fn list_agent_images(_: Cx, _: ListAgentImages) -> Reply<Vec<AgentImage>> {
    Ok(Vec::new())
}

/// Part of a media file on one of the caller's machines; nothing is kept.
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

/// The models `list.agent` can run on `list.machine_id`; a node too old to say gives `Fixed`.
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
        beep_mesh::machine::agent_models,
    ))
}

/// The branch or worktree a start asks `node` to make; refused when the node is too old to make it.
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
