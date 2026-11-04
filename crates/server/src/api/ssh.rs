//! Adding a machine over the ssh of another of the caller's machines, and
//! where a machine looks for git checkouts.

use super::Reply;
use crate::{
    api::router::Cx,
    nodes::{self, Online, TokenRefusal},
    protocol::*,
};
use std::time::Duration;
use tokio::time::{Instant, sleep};

/// How long a linked machine has to come online.
const ONLINE_WAIT: Duration = Duration::from_secs(30);
const ONLINE_POLL: Duration = Duration::from_millis(500);

pub(super) async fn list_ssh_hosts(
    cx: Cx,
    ListSshHosts { machine_id }: ListSshHosts,
) -> Reply<Vec<String>> {
    let online = ssh_node(&cx, &machine_id).await?;
    online
        .ssh_hosts()
        .await
        .map_err(|e| ApiError::bad_request(format!("{e:#}")))
}

/// A failed step is a reply, with its output; only a machine that cannot
/// run it is an error.
pub(super) async fn add_machine_over_ssh(
    cx: Cx,
    AddMachineOverSsh {
        machine_id,
        target,
        search,
    }: AddMachineOverSsh,
) -> Reply<SshAdd> {
    let (target, search) = (target.trim(), search.trim());
    if target.is_empty() {
        return Err(ApiError::bad_request("Name a host, such as user@host."));
    }
    let online = ssh_node(&cx, &machine_id).await?;
    // An older node installs without passing the folder on.
    if !search.is_empty() && !online.runs_search() {
        return Err(ApiError::bad_request(format!(
            "Update beep-node on {} to pick where the new machine looks for projects.",
            online.name
        )));
    }
    let made = match nodes::create_token(&cx.app, &cx.user, None).await {
        Ok(made) => made,
        Err(TokenRefusal::Failed(e)) => return Err(e.into()),
        Err(refusal) => return Err(ApiError::bad_request(refusal.to_string())),
    };
    let hash = made.token.hash();
    let done = online.add_over_ssh(target, &made.token, search).await;
    let failed = |system: String, error: String, output: Vec<String>| SshAdd {
        machine_id: None,
        system,
        error,
        output,
    };
    let result = match done {
        Ok(result) => result,
        Err(e) => {
            revoke(&cx, &hash).await;
            return Ok(failed(String::new(), format!("{e:#}"), Vec::new()));
        }
    };
    if let Some(failure) = result.failure {
        revoke(&cx, &hash).await;
        return Ok(failed(result.system, failure.reason, failure.output));
    }
    let deadline = Instant::now() + ONLINE_WAIT;
    loop {
        if let Some(Some(id)) = cx.user.node_token_claimer(&hash).await?
            && cx.app.nodes.online(&id).is_some()
        {
            log::info!("machine added over ssh; via={machine_id} machine={id}");
            return Ok(SshAdd {
                machine_id: Some(id),
                system: result.system,
                error: String::new(),
                output: Vec::new(),
            });
        }
        if Instant::now() >= deadline {
            return Ok(failed(
                result.system,
                format!("beep-node is set up on {target} but has not come online."),
                Vec::new(),
            ));
        }
        sleep(ONLINE_POLL).await;
    }
}

/// One of the caller's online machines that runs `ssh/1`.
async fn ssh_node(cx: &Cx, machine_id: &str) -> Result<Online, ApiError> {
    let (name, online) = owned_online(cx, machine_id).await?;
    if !online.runs_ssh() {
        return Err(ApiError::bad_request(format!(
            "Update beep-node on {name} to add machines over ssh from it."
        )));
    }
    Ok(online)
}

/// Where `machine_id` looks for git checkouts.
pub(super) async fn get_machine_search(
    cx: Cx,
    GetMachineSearch { machine_id }: GetMachineSearch,
) -> Reply<String> {
    let online = search_node(&cx, &machine_id).await?;
    online
        .search()
        .await
        .map_err(|e| ApiError::bad_request(format!("{e:#}")))
}

/// Has `machine_id` look for git checkouts in `search` and walk it again.
pub(super) async fn set_machine_search(
    cx: Cx,
    SetMachineSearch { machine_id, search }: SetMachineSearch,
) -> Reply<String> {
    let online = search_node(&cx, &machine_id).await?;
    online
        .set_search(search.trim())
        .await
        .map_err(|e| ApiError::bad_request(format!("{e:#}")))
}

/// One of the caller's online machines that runs `search/1`.
async fn search_node(cx: &Cx, machine_id: &str) -> Result<Online, ApiError> {
    let (name, online) = owned_online(cx, machine_id).await?;
    if !online.runs_search() {
        return Err(ApiError::bad_request(format!(
            "Update beep-node on {name} to pick where it looks for projects."
        )));
    }
    Ok(online)
}

/// One of the caller's online machines, with its name. Another account's
/// machine is `NotFound`, as one that does not exist.
async fn owned_online(cx: &Cx, machine_id: &str) -> Result<(String, Online), ApiError> {
    let owned = cx
        .user
        .nodes()
        .await?
        .into_iter()
        .find(|node| node.endpoint == machine_id)
        .ok_or(ApiError::NotFound {})?;
    let online = cx
        .app
        .nodes
        .online(machine_id)
        .filter(|online| online.user == cx.user.id())
        .ok_or_else(|| ApiError::bad_request(format!("{} is offline.", owned.name)))?;
    Ok((owned.name, online))
}

/// An unused token stops working; a revoke that fails only leaves it to
/// expire.
async fn revoke(cx: &Cx, hash: &str) {
    if let Err(e) = cx.user.revoke_node_token(hash).await {
        log::warn!("setup token not revoked after a failed ssh add: {e:#}");
    }
}
