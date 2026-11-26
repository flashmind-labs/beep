//! Settings, Credentials: the account's GitHub tokens, Forgejo servers and API keys.
//! Secrets go into the mesh once, for every machine, and never come back; the
//! bar sees only their last four characters.

use crate::{
    beep::epoch_ms,
    error::Result,
    fleet::projects::{FleetState, desk},
};
use beep_fleet::Session;
use beep_proto::{ApiKey, Credential, CredentialCheck, GitHubAccount, SecretText, Service};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use strum::VariantArray;

/// A credential as the bar shows it.
#[derive(Debug, Serialize)]
pub struct CredentialView {
    id: String,
    #[serde(flatten)]
    service: Service,
    last4: String,
    account: Option<String>,
    env_var: String,
    /// Milliseconds since the Unix epoch.
    created_at: u64,
    /// It can no longer be opened: the bar asks for it again.
    unreadable: bool,
}

impl From<Credential> for CredentialView {
    fn from(credential: Credential) -> Self {
        Self {
            id: credential.id,
            service: credential.service,
            last4: credential.last4,
            account: credential.account,
            env_var: credential.env_var,
            created_at: epoch_ms(credential.created_at),
            unreadable: credential.unreadable,
        }
    }
}

/// One GitHub account as the bar shows it.
#[derive(Debug, Serialize)]
pub struct GitHubAccountView {
    id: String,
    login: String,
    orgs: Vec<String>,
    default: bool,
    last4: String,
    /// Milliseconds since the Unix epoch.
    created_at: u64,
    unreadable: bool,
}

impl From<GitHubAccount> for GitHubAccountView {
    fn from(account: GitHubAccount) -> Self {
        Self {
            id: account.id,
            login: account.login,
            orgs: account.orgs,
            default: account.default,
            last4: account.last4,
            created_at: epoch_ms(account.created_at),
            unreadable: account.unreadable,
        }
    }
}

/// What the bar sends to save one.
#[derive(Deserialize)]
pub struct NewCredential {
    #[serde(flatten)]
    service: Service,
    secret: String,
}

#[tauri::command]
pub async fn list_credentials(fleet: FleetState<'_>) -> Result<Vec<CredentialView>> {
    let credentials = desk(&fleet).await?.credentials();
    Ok(credentials.into_iter().map(CredentialView::from).collect())
}

/// A provider an API key can be saved for.
#[derive(Debug, Serialize)]
pub struct ApiKeyView {
    provider: ApiKey,
    name: &'static str,
    env_var: &'static str,
}

/// The providers, in the order Settings lists them.
#[tauri::command]
pub fn api_providers() -> Vec<ApiKeyView> {
    ApiKey::VARIANTS
        .iter()
        .map(|&provider| ApiKeyView {
            provider,
            name: provider.human_readable_name(),
            env_var: provider.env_var(),
        })
        .collect()
}

/// Checks and saves one; a refusal comes back as the host's reason.
#[tauri::command]
pub async fn save_credential(
    fleet: FleetState<'_>,
    credential: NewCredential,
) -> Result<CredentialView> {
    let NewCredential { service, secret } = credential;
    save(&fleet, service, SecretText::new(secret)).await
}

/// Checks one with its host, then keeps it for every machine. A GitHub token
/// is saved as the account it signs in as, beside the others.
pub(crate) async fn save(
    fleet: &Arc<Session>,
    service: Service,
    secret: SecretText,
) -> Result<CredentialView> {
    let saved = desk(fleet)
        .await?
        .save_credential(&service, &secret)
        .await?;
    // What was read with the old tokens is not served to the new ones.
    fleet.forget();
    Ok(saved.into())
}

/// What checking a Forgejo token found, as the bar shows it.
#[derive(Debug, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum CheckView {
    Valid {
        account: String,
        machine: Option<String>,
    },
    Refused {
        machine: Option<String>,
        message: String,
    },
    Unreachable {
        message: String,
    },
}

impl From<CredentialCheck> for CheckView {
    fn from(check: CredentialCheck) -> Self {
        match check {
            CredentialCheck::Valid { account, machine } => Self::Valid { account, machine },
            CredentialCheck::Refused { machine, message } => Self::Refused { machine, message },
            CredentialCheck::Unreachable { message } => Self::Unreachable { message },
        }
    }
}

/// Checks a saved Forgejo token from this Mac, else from the machines with
/// teacli.
#[tauri::command]
pub async fn check_credential(fleet: FleetState<'_>, id: String) -> Result<CheckView> {
    Ok(desk(&fleet).await?.check_credential(&id).await?.into())
}

#[tauri::command]
pub async fn remove_credential(fleet: FleetState<'_>, id: String) -> Result<()> {
    desk(&fleet).await?.remove_credential(&id).await?;
    fleet.forget();
    Ok(())
}

/// The GitHub accounts, the default first.
#[tauri::command]
pub async fn list_github_accounts(fleet: FleetState<'_>) -> Result<Vec<GitHubAccountView>> {
    let accounts = desk(&fleet).await?.github_accounts();
    Ok(accounts.into_iter().map(GitHubAccountView::from).collect())
}

/// Makes one GitHub account the one machines get as GH_TOKEN.
#[tauri::command]
pub async fn set_default_github_account(fleet: FleetState<'_>, id: String) -> Result<()> {
    desk(&fleet).await?.set_default_github(&id).await?;
    fleet.forget();
    Ok(())
}
