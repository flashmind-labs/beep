//! The caller's credentials in Settings; secrets never come back.

use super::Reply;
use crate::{
    account::credentials::{
        Checked, Checker, Slot, last4,
        machines::{MAX_MACHINES, Reach, check_from},
    },
    api::router::Cx,
    nodes::SHELL,
    protocol::{
        ApiError, ApiProvider, CheckCredential, Credential, CredentialCheck, CredentialKind,
        GITHUB_ENV_VAR, GitHubAccount, ListCredentials, ListGitHubAccounts, RemoveCredential,
        SaveCredential, SaveGitHubAccount, SecretText, SetDefaultGitHubAccount,
    },
    store::{CredentialRow, GitHubAccountRow, NewCredential, NewGitHubAccount},
};
use secrecy::{ExposeSecret, SecretString};
use std::{sync::Arc, time::SystemTime};

/// The longest secret taken.
const MAX_SECRET: usize = 4096;

pub(super) async fn list_credentials(cx: Cx, _: ListCredentials) -> Reply<Vec<Credential>> {
    Ok(cx
        .user
        .credentials(&cx.app.credential_key)
        .await?
        .into_iter()
        .map(credential)
        .collect())
}

pub(super) async fn save_credential(
    cx: Cx,
    SaveCredential {
        kind,
        secret,
        to_machines,
    }: SaveCredential,
) -> Reply<Credential> {
    let slot = Slot::from_wire(&kind).map_err(|e| ApiError::bad_request(e.to_string()))?;
    let secret = trimmed(&secret)?;
    if slot == Slot::GitHub {
        // An app from before several accounts replaces the default one.
        let saved = save_github(&cx, &secret, true).await?;
        return Ok(github_credential(saved));
    }
    let account = check(&slot, &secret).await?;
    let shown = last4(secret.expose_secret());
    let row = cx
        .user
        .save_credential(
            &cx.app.credential_key,
            NewCredential {
                slot: &slot,
                secret: &secret,
                last4: &shown,
                account: account.as_deref(),
                to_machines: to_machines || slot.always_to_machines(),
            },
        )
        .await?;
    cx.app.nodes.credentials_changed(cx.user.id());
    Ok(credential(row))
}

pub(super) async fn remove_credential(
    cx: Cx,
    RemoveCredential { id }: RemoveCredential,
) -> Reply<()> {
    if !cx.user.remove_credential(&id).await? {
        return Err(ApiError::NotFound {});
    }
    cx.app.nodes.credentials_changed(cx.user.id());
    Ok(())
}

pub(super) async fn check_credential(
    cx: Cx,
    CheckCredential { id }: CheckCredential,
) -> Reply<CredentialCheck> {
    let login = cx
        .user
        .git_logins(&cx.app.credential_key)
        .await?
        .into_iter()
        .find(|login| login.id == id);
    let Some(login) = login else {
        return Err(ApiError::NotFound {});
    };
    let Slot::Forgejo(url) = &login.slot else {
        return Err(ApiError::NotFound {});
    };
    let from_server = if url.is_public().await {
        Checker::new()?.forgejo(url, &login.secret).await
    } else {
        Checked::Unreachable
    };
    let checked = match from_server {
        Checked::Valid {
            login: Some(account),
        } => CredentialCheck::Valid {
            account,
            machine: None,
        },
        // A host that answers without a login says nothing more from a machine.
        Checked::Valid { login: None } => {
            return Ok(CredentialCheck::Unreachable {
                message: format!("{} did not say who the token signs in as.", url.host()),
            });
        }
        Checked::Refused(message) => CredentialCheck::Refused {
            machine: None,
            message,
        },
        Checked::Unreachable => check_from(&reaches(&cx).await?, url).await,
    };
    if let CredentialCheck::Valid { account, .. } = &checked
        && login.account.as_deref() != Some(account.as_str())
    {
        cx.user.set_forgejo_account(&id, account).await?;
    }
    Ok(checked)
}

/// The caller's online machines that run commands and report teacli, at most [`MAX_MACHINES`].
async fn reaches(cx: &Cx) -> Reply<Vec<Reach>> {
    let mut out = Vec::new();
    for row in cx.user.nodes().await? {
        let has_teacli = row
            .env
            .as_ref()
            .is_some_and(|env| env.tools.iter().any(|tool| tool.name == "teacli"));
        if !has_teacli {
            continue;
        }
        let Some(online) = cx
            .app
            .nodes
            .online(&row.endpoint)
            .filter(|online| online.user == cx.user.id() && online.offers(SHELL))
        else {
            continue;
        };
        out.push(Reach {
            name: row.name,
            shell: Arc::new(online),
        });
        if out.len() == MAX_MACHINES {
            break;
        }
    }
    Ok(out)
}

pub(super) async fn list_github_accounts(
    cx: Cx,
    _: ListGitHubAccounts,
) -> Reply<Vec<GitHubAccount>> {
    Ok(cx
        .user
        .github_accounts(&cx.app.credential_key)
        .await?
        .into_iter()
        .map(github_account)
        .collect())
}

pub(super) async fn save_github_account(
    cx: Cx,
    SaveGitHubAccount { secret }: SaveGitHubAccount,
) -> Reply<GitHubAccount> {
    let secret = trimmed(&secret)?;
    Ok(github_account(save_github(&cx, &secret, false).await?))
}

pub(super) async fn set_default_github_account(
    cx: Cx,
    SetDefaultGitHubAccount { id }: SetDefaultGitHubAccount,
) -> Reply<()> {
    if !cx.user.set_default_github(&id).await? {
        return Err(ApiError::NotFound {});
    }
    cx.app.nodes.credentials_changed(cx.user.id());
    Ok(())
}

/// `secret` trimmed, refused when it does not look like a token.
pub(super) fn trimmed(secret: &SecretText) -> Reply<SecretString> {
    let secret = SecretString::from(secret.expose().trim());
    let text = secret.expose_secret();
    if text.is_empty() {
        return Err(ApiError::bad_request("Paste the token or key first."));
    }
    if text.len() > MAX_SECRET || text.chars().any(char::is_whitespace) {
        return Err(ApiError::bad_request(
            "That does not look like a token or key: it has spaces or is too long.",
        ));
    }
    Ok(secret)
}

/// Checks a GitHub token, reads its orgs and saves it as its account.
async fn save_github(
    cx: &Cx,
    secret: &SecretString,
    replace_default: bool,
) -> Reply<GitHubAccountRow> {
    let checker = Checker::new()?;
    let login = match check(&Slot::GitHub, secret).await? {
        Some(login) => login,
        None => {
            return Err(ApiError::bad_request(
                "GitHub did not say who the token signs in as. Try again.",
            ));
        }
    };
    let orgs = checker.github_orgs(secret).await;
    let shown = last4(secret.expose_secret());
    let saved = cx
        .user
        .save_github_account(
            &cx.app.credential_key,
            NewGitHubAccount {
                secret,
                last4: &shown,
                login: &login,
                orgs: &orgs,
                replace_default,
            },
        )
        .await?;
    cx.app.nodes.credentials_changed(cx.user.id());
    Ok(saved)
}

/// Asks the host whether `secret` works and who it signs in as. A private or unreachable
/// Forgejo, or unreachable OpenRouter, is saved unchecked: the server must not probe its network.
async fn check(slot: &Slot, secret: &SecretString) -> Reply<Option<String>> {
    let checked = match slot {
        Slot::GitHub => Checker::new()?.github(secret).await,
        Slot::Forgejo(url) if !url.is_public().await => return Ok(None),
        Slot::Forgejo(url) => Checker::new()?.forgejo(url, secret).await,
        Slot::ApiKey(ApiProvider::OpenRouter) => Checker::new()?.openrouter(secret).await,
        Slot::ApiKey(_) | Slot::AgentToken(_) => return Ok(None),
    };
    match checked {
        Checked::Valid { login } => Ok(login),
        Checked::Refused(why) => Err(ApiError::bad_request(why)),
        Checked::Unreachable if *slot == Slot::GitHub => Err(ApiError::bad_request(
            "Could not reach GitHub to check the token. Try again.",
        )),
        Checked::Unreachable => Ok(None),
    }
}

fn credential(row: CredentialRow) -> Credential {
    Credential {
        env_var: row.env_var,
        kind: row.kind,
        id: row.id,
        last4: row.last4,
        account: row.account,
        to_machines: row.to_machines,
        created_at: SystemTime::from(row.created_at),
        unreadable: row.unreadable,
    }
}

/// A GitHub account as the one GitHub row of `ListCredentials`.
fn github_credential(row: GitHubAccountRow) -> Credential {
    Credential {
        id: row.id,
        kind: CredentialKind::GitHub {},
        last4: row.last4,
        account: Some(row.login),
        env_var: GITHUB_ENV_VAR.to_owned(),
        to_machines: true,
        created_at: SystemTime::from(row.created_at),
        unreadable: row.unreadable,
    }
}

fn github_account(row: GitHubAccountRow) -> GitHubAccount {
    GitHubAccount {
        id: row.id,
        login: row.login,
        orgs: row.orgs,
        default: row.default,
        last4: row.last4,
        created_at: SystemTime::from(row.created_at),
        unreadable: row.unreadable,
    }
}
