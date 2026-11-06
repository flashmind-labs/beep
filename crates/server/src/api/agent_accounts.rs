//! Agent accounts: a sealed Claude Code token handed out as `CLAUDE_CODE_OAUTH_TOKEN`, or a
//! Codex API key. Codex's device sign-in runs on a machine, which a desktop reaches now.

use super::{Reply, credentials::trimmed, own_nodes::retired};
use crate::{
    api::router::Cx,
    protocol::*,
    store::{AgentTokenRow, NewCredential},
};
use beep_projects::credentials::{Slot, last4};
use secrecy::ExposeSecret;
use std::{str::FromStr, time::SystemTime};

pub(super) async fn get_agent_accounts(cx: Cx, _: GetAgentAccounts) -> Reply<AgentAccounts> {
    let tokens = cx.user.agent_tokens(&cx.app.credential_key).await?;
    let token = |agent: AgentAccount| {
        tokens
            .iter()
            .find(|row| row.agent == agent)
            .map(agent_token)
    };
    Ok(AgentAccounts {
        claude: token(AgentAccount::Claude),
        codex_key: token(AgentAccount::Codex),
        machines: Vec::new(),
    })
}

pub(super) async fn start_codex_login(_: Cx, _: StartCodexLogin) -> Reply<CodexLogin> {
    retired()
}

pub(super) async fn get_codex_login(_: Cx, _: GetCodexLogin) -> Reply<CodexLoginState> {
    Ok(CodexLoginState::Gone {})
}

pub(super) async fn cancel_codex_login(_: Cx, _: CancelCodexLogin) -> Reply<()> {
    retired()
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
    Ok(())
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
