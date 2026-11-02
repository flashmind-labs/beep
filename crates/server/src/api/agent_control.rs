//! Starting and steering coding-agent sessions from the app: the machines
//! that can run one, starting one (on a named machine or by Auto), reading
//! one in full and sending it a message, a stop or a close.

use super::{
    Reply,
    agent_auto::{self, Candidate, Wants},
    agent_sessions::{
        Host, IN_FLIGHT, NODE_LIMIT, beep_session, external_session, limited, linked_keys,
    },
    machines::machine_capability,
};
use crate::{
    nodes::{Follower, Online, agent_name},
    protocol::{
        node::agent::{self, PlanStatus, Request, SessionInfo},
        *,
    },
    router::Cx,
    tools::machines::command_name,
};
use futures::{StreamExt, stream};
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
/// store lists them. A machine whose key was unlinked is left out.
pub(super) async fn live_hosts(cx: &Cx) -> Result<Vec<Live>, ApiError> {
    let keys = linked_keys(cx).await?;
    Ok(cx
        .user
        .nodes()
        .await?
        .into_iter()
        .filter(|row| keys.contains(&row.endpoint))
        .filter_map(|row| {
            let online = cx
                .app
                .nodes
                .online(&row.endpoint)
                .filter(|online| online.user == cx.user.id())?;
            let agents: Vec<String> = online
                .offers
                .iter()
                .filter_map(|offer| agent::agent_of(offer))
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
async fn live_host(cx: &Cx, machine_id: &str) -> Result<Live, ApiError> {
    live_hosts(cx)
        .await?
        .into_iter()
        .find(|live| live.host.machine_id == machine_id)
        .ok_or(ApiError::NotFound {})
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
    Ok(StartedAgent {
        session: beep_session(&live.host(), link.now().info),
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
    let cwd = start
        .cwd
        .map(|cwd| cwd.trim().to_owned())
        .filter(|cwd| !cwd.is_empty())
        .or_else(|| live.online.workspace.clone());
    let link = live
        .online
        .start_agent(&agent, cwd, prompt)
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
    let live = live_host(&cx, &get.machine_id).await?;
    let host = live.host();
    let listed = limited(live.online.agent_sessions()).await?;
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
    match timeout(NODE_LIMIT, live.online.transcript(&get.session, last)).await {
        Ok(Ok((entries, earlier))) => {
            view.entries = entries.into_iter().map(entry).collect();
            view.earlier = earlier;
        }
        Ok(Err(e)) => log::warn!("transcript not read; session={} error={e:#}", get.session),
        Err(_) => log::warn!("transcript not read in time; session={}", get.session),
    }
    Ok(view)
}

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

fn entry(entry: agent::Entry) -> AgentEntry {
    let kind = match entry.kind {
        agent::EntryKind::User => AgentEntryKind::User {},
        agent::EntryKind::Agent => AgentEntryKind::Agent {},
        agent::EntryKind::Tool => AgentEntryKind::Tool {},
        agent::EntryKind::Question => AgentEntryKind::Question {},
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
    link.send(request).await.map_err(|e| {
        let error = format!("{e:#}");
        // A node from before images does not know the request.
        if images && error.contains("unknown request") {
            ApiError::bad_request(
                "beep-node on that machine is too old to send images. Update it, or send text.",
            )
        } else {
            ApiError::bad_request(error)
        }
    })?;
    Ok(())
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
        AgentSteer::Cancel {} => Request::Cancel {},
        AgentSteer::Close {} => Request::Close {},
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
