//! Agent accounts: a sealed Claude Code token handed out as `CLAUDE_CODE_OAUTH_TOKEN`,
//! and Codex's device sign-in run on each machine as a background task (or an API key).

use super::{
    Reply,
    agent_sessions::{IN_FLIGHT, NODE_LIMIT},
    credentials::trimmed,
    own_nodes::{linked_keys, online},
};
use crate::{
    api::router::Cx,
    nodes::Online,
    protocol::{node::EnvReport, *},
    store::{AgentTokenRow, NewCredential},
};
use beep_mesh::{PeerClient, machine::codex_login};
use beep_projects::credentials::Slot;
use beep_projects::credentials::last4;
use futures::{StreamExt, stream};
use secrecy::ExposeSecret;
use std::{
    str::FromStr,
    time::{Duration, SystemTime},
};
use tokio::time::timeout;

/// What tells whether Codex is signed in on a machine.
const CODEX_STATUS: &str = "codex login status";
/// How long `codex login status` may take.
const STATUS_LIMIT: Duration = Duration::from_secs(5);

pub(super) async fn get_agent_accounts(cx: Cx, _: GetAgentAccounts) -> Reply<AgentAccounts> {
    let tokens = cx.user.agent_tokens(&cx.app.credential_key).await?;
    let token = |agent: AgentAccount| {
        tokens
            .iter()
            .find(|row| row.agent == agent)
            .map(agent_token)
    };
    let keys = linked_keys(&cx).await?;
    // Futures made up front: a closure inside the stream would make the reply not `Send`.
    let asks: Vec<_> = cx
        .user
        .nodes()
        .await?
        .into_iter()
        .filter(|node| keys.contains(&node.endpoint))
        .filter_map(|node| {
            let env = node.env.unwrap_or_default();
            let tools = Tools::of(&env);
            if !tools.claude && !tools.codex {
                return None;
            }
            let online = online(&cx, &node.endpoint);
            Some(machine(node.endpoint, node.name, env, tools, online))
        })
        .collect();
    let mut machines: Vec<AgentMachine> = stream::iter(asks).buffered(IN_FLIGHT).collect().await;
    machines.sort_by_key(|machine| machine.machine_name.to_lowercase());
    Ok(AgentAccounts {
        claude: token(AgentAccount::Claude),
        codex_key: token(AgentAccount::Codex),
        machines,
    })
}

pub(super) async fn save_agent_token(
    cx: Cx,
    SaveAgentToken { agent, secret }: SaveAgentToken,
) -> Reply<AgentToken> {
    let agent = agent_named(&agent)?;
    let secret = trimmed(&secret)?;
    if let Some(why) = refusal(agent, secret.expose_secret()) {
        return Err(ApiError::bad_request(why));
    }
    let shown = last4(secret.expose_secret());
    let row = cx
        .user
        .save_credential(
            &cx.app.credential_key,
            NewCredential {
                slot: &Slot::AgentToken(agent),
                secret: &secret,
                last4: &shown,
                account: None,
                to_machines: true,
            },
        )
        .await?;
    cx.app.nodes.credentials_changed(cx.user.id());
    Ok(AgentToken {
        last4: row.last4,
        created_at: SystemTime::from(row.created_at),
        unreadable: false,
    })
}

pub(super) async fn revoke_agent_token(
    cx: Cx,
    RevokeAgentToken { agent }: RevokeAgentToken,
) -> Reply<()> {
    let agent = agent_named(&agent)?;
    if !cx.user.remove_agent_token(agent).await? {
        return Err(ApiError::NotFound {});
    }
    cx.app.nodes.credentials_changed(cx.user.id());
    Ok(())
}

pub(super) async fn start_codex_login(
    cx: Cx,
    StartCodexLogin { machine_id }: StartCodexLogin,
) -> Reply<CodexLogin> {
    let node = linked_online(&cx, &machine_id).await?;
    codex_login::start(&PeerClient::new(node.connection.clone()))
        .await
        .map_err(|e| ApiError::bad_request(format!("{}: {e}", node.name)))
}

pub(super) async fn get_codex_login(
    cx: Cx,
    GetCodexLogin { machine_id, task }: GetCodexLogin,
) -> Reply<CodexLoginState> {
    if !linked_keys(&cx).await?.contains(&machine_id) {
        return Err(ApiError::NotFound {});
    }
    let Some(node) = online(&cx, &machine_id) else {
        return Ok(CodexLoginState::Gone {});
    };
    codex_login::state(&PeerClient::new(node.connection.clone()), task)
        .await
        .map_err(|e| ApiError::bad_request(format!("{}: {e}", node.name)))
}

pub(super) async fn cancel_codex_login(
    cx: Cx,
    CancelCodexLogin { machine_id, task }: CancelCodexLogin,
) -> Reply<()> {
    if !linked_keys(&cx).await?.contains(&machine_id) {
        return Err(ApiError::NotFound {});
    }
    let Some(node) = online(&cx, &machine_id) else {
        return Ok(());
    };
    codex_login::cancel(&PeerClient::new(node.connection.clone()), task)
        .await
        .map_err(|e| ApiError::bad_request(format!("{}: {e}", node.name)))
}

/// Which agents a machine has, from its last report.
struct Tools {
    claude: bool,
    codex: bool,
}

impl Tools {
    fn of(env: &EnvReport) -> Self {
        let has = |name: &str| env.tools.iter().any(|tool| tool.name == name);
        Self {
            claude: has("claude"),
            codex: has("codex"),
        }
    }
}

/// One machine's row; what it does not answer in time is left unknown.
async fn machine(
    id: String,
    name: String,
    env: EnvReport,
    tools: Tools,
    online: Option<Online>,
) -> AgentMachine {
    let (claude_sessions, codex_signed_in) = match &online {
        Some(node) => {
            let sessions = async {
                if !tools.claude {
                    return 0;
                }
                match timeout(NODE_LIMIT, node.agent_sessions()).await {
                    Ok(Ok(sessions)) => {
                        let count = sessions
                            .iter()
                            .filter(|session| session.agent == AgentAccount::Claude.slug())
                            .count();
                        u32::try_from(count).unwrap_or(u32::MAX)
                    }
                    _ => 0,
                }
            };
            let codex = async {
                if !tools.codex {
                    return None;
                }
                codex_signed_in(node)
                    .await
                    .or_else(|| reported_codex_login(&env))
            };
            tokio::join!(sessions, codex)
        }
        None => (0, None),
    };
    AgentMachine {
        machine_id: id,
        machine_name: name,
        online: online.is_some(),
        claude: tools.claude,
        codex: tools.codex,
        claude_sessions,
        codex_signed_in,
    }
}

/// Whether `codex login status` says signed in; `None` when it could not be asked.
async fn codex_signed_in(node: &Online) -> Option<bool> {
    let mut output = String::new();
    let exited = node
        .exec(CODEX_STATUS, STATUS_LIMIT, &mut |text| {
            output.push_str(text)
        })
        .await;
    match exited {
        Ok(exited) if !exited.timed_out => {
            Some(exited.exit_code == Some(0) && !output.contains("Not logged in"))
        }
        Ok(_) => None,
        Err(e) => {
            log::debug!("codex login status on {}: {e:#}", node.name);
            None
        }
    }
}

/// The Codex sign-in the machine last reported.
fn reported_codex_login(env: &EnvReport) -> Option<bool> {
    env.logins
        .iter()
        .find(|login| login.tool == AgentAccount::Codex.slug())
        .map(|login| login.signed_in)
}

fn agent_token(row: &AgentTokenRow) -> AgentToken {
    AgentToken {
        last4: row.last4.clone(),
        created_at: SystemTime::from(row.created_at),
        unreadable: row.unreadable,
    }
}

fn agent_named(slug: &str) -> Reply<AgentAccount> {
    AgentAccount::from_str(slug)
        .map_err(|_| ApiError::bad_request(format!("Beep keeps no token for {slug:?}.")))
}

/// Why `secret` is not a token for `agent`, when it plainly is not.
fn refusal(agent: AgentAccount, secret: &str) -> Option<&'static str> {
    match agent {
        AgentAccount::Claude if secret.starts_with("sk-ant-api") => Some(
            "That is an Anthropic API key. Paste the token `claude setup-token` prints, or add \
             the key under Credentials.",
        ),
        AgentAccount::Claude | AgentAccount::Codex => None,
    }
}

/// The caller's linked machine `endpoint`, online now; `NotFound` otherwise.
async fn linked_online(cx: &Cx, endpoint: &str) -> Reply<Online> {
    if !linked_keys(cx).await?.contains(endpoint) {
        return Err(ApiError::NotFound {});
    }
    online(cx, endpoint).ok_or(ApiError::NotFound {})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_api_key_is_not_taken_as_a_claude_token() {
        assert!(refusal(AgentAccount::Claude, "sk-ant-api03-abcdef").is_some());
        assert_eq!(refusal(AgentAccount::Claude, "sk-ant-oat01-abcdef"), None);
        assert_eq!(refusal(AgentAccount::Codex, "sk-proj-abcdef"), None);
    }
}
