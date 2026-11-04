//! Adding a machine over the ssh of another of the caller's machines.

use super::Reply;
use crate::{
    nodes::{self, Online, TokenRefusal},
    protocol::*,
    router::Cx,
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
    AddMachineOverSsh { machine_id, target }: AddMachineOverSsh,
) -> Reply<SshAdd> {
    let target = target.trim();
    if target.is_empty() {
        return Err(ApiError::bad_request("Name a host, such as user@host."));
    }
    let online = ssh_node(&cx, &machine_id).await?;
    let made = match nodes::create_token(&cx.app, &cx.user, None).await {
        Ok(made) => made,
        Err(TokenRefusal::Failed(e)) => return Err(e.into()),
        Err(refusal) => return Err(ApiError::bad_request(refusal.to_string())),
    };
    let hash = made.token.hash();
    let done = online.add_over_ssh(target, &made.token).await;
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

/// One of the caller's online machines that runs `ssh/1`. Another
/// account's machine is `NotFound`, as one that does not exist.
async fn ssh_node(cx: &Cx, machine_id: &str) -> Result<Online, ApiError> {
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
    if !online.runs_ssh() {
        return Err(ApiError::bad_request(format!(
            "Update beep-node on {} to add machines over ssh from it.",
            owned.name
        )));
    }
    Ok(online)
}

/// An unused token stops working; a revoke that fails only leaves it to
/// expire.
async fn revoke(cx: &Cx, hash: &str) {
    if let Err(e) = cx.user.revoke_node_token(hash).await {
        log::warn!("setup token not revoked after a failed ssh add: {e:#}");
    }
}
