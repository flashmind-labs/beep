//! Picks the machine a coding agent starts on when the user did not.

use super::super::{
    agent_auto::{self, Candidate, Wants},
    agent_sessions::{IN_FLIGHT, NODE_LIMIT, limited},
};
use super::Live;
use crate::{
    nodes::{Online, agent_name},
    protocol::{
        node::agent::{self},
        *,
    },
};
use futures::{StreamExt, stream};
use tokio::time::timeout;

/// The machine Auto picks for `start`, and why.
pub(super) async fn auto(
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
        // Futures made up front: a closure inside the stream would make the reply not `Send`.
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

/// Whether `folder` is on `online`. Unasked, a folder under another system's home root is missing.
pub(super) async fn probed(
    at: usize,
    online: Online,
    home: Option<String>,
    folder: &str,
) -> (usize, bool) {
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
pub(super) fn folder_test(folder: &str) -> Option<String> {
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
pub(super) fn plausible(folder: &str, home: Option<&str>) -> bool {
    let Some(home) = home else {
        return true;
    };
    ["/Users/", "/home/"]
        .iter()
        .all(|root| !folder.starts_with(root) || home.starts_with(root.trim_end_matches('/')))
}

pub(super) async fn counted(at: usize, online: Online) -> (usize, u32) {
    (at, running(&online).await)
}

/// Sessions running on `online`; a node that does not answer counts as busy.
pub(super) async fn running(online: &Online) -> u32 {
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
