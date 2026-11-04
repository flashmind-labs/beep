//! Settings, Credentials: the account's GitHub token, Forgejo servers and
//! API keys. Secrets go into the mesh once, for every machine, and never
//! come back; the bar sees only their last four characters. A key only the
//! server uses is saved there.

use crate::{
    beep::{Beep, Service, epoch_ms},
    error::{DesktopError, Result},
    fleet::{
        Fleet,
        projects::{FleetState, desk},
    },
};
use beep_fleet::FleetError;
use beep_proto::{
    ApiError, ApiProvider, Credential, CredentialCheck, CredentialKind, GITHUB_ENV_VAR,
    GitHubAccount, SaveCredential, SecretText,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// What a credential is for, as the bar names it.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Kind {
    Github,
    Forgejo { url: String },
    ApiKey { provider: String },
}

impl From<CredentialKind> for Kind {
    fn from(kind: CredentialKind) -> Self {
        match kind {
            CredentialKind::GitHub {} => Self::Github,
            CredentialKind::Forgejo { url } => Self::Forgejo { url },
            CredentialKind::ApiKey { provider } => Self::ApiKey { provider },
        }
    }
}

impl From<Kind> for CredentialKind {
    fn from(kind: Kind) -> Self {
        match kind {
            Kind::Github => Self::GitHub {},
            Kind::Forgejo { url } => Self::Forgejo { url },
            Kind::ApiKey { provider } => Self::ApiKey { provider },
        }
    }
}

/// A credential as the bar shows it.
#[derive(Debug, Serialize)]
pub struct CredentialView {
    id: String,
    #[serde(flatten)]
    kind: Kind,
    last4: String,
    account: Option<String>,
    env_var: String,
    to_machines: bool,
    /// Milliseconds since the Unix epoch.
    created_at: u64,
    /// The server can no longer open it: the bar asks for it again.
    unreadable: bool,
}

impl From<Credential> for CredentialView {
    fn from(credential: Credential) -> Self {
        Self {
            id: credential.id,
            kind: credential.kind.into(),
            last4: credential.last4,
            account: credential.account,
            env_var: credential.env_var,
            to_machines: credential.to_machines,
            created_at: epoch_ms(credential.created_at),
            unreadable: credential.unreadable,
        }
    }
}

impl From<GitHubAccount> for CredentialView {
    fn from(account: GitHubAccount) -> Self {
        Self {
            id: account.id,
            kind: Kind::Github,
            last4: account.last4,
            account: Some(account.login),
            env_var: GITHUB_ENV_VAR.to_owned(),
            to_machines: true,
            created_at: epoch_ms(account.created_at),
            unreadable: account.unreadable,
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

/// A provider an API key can be added for.
#[derive(Debug, Serialize)]
pub struct ProviderView {
    slug: &'static str,
    name: &'static str,
    env_var: &'static str,
}

/// What the bar sends to save one.
#[derive(Deserialize)]
pub struct NewCredential {
    #[serde(flatten)]
    kind: Kind,
    secret: String,
    #[serde(default)]
    to_machines: bool,
}

/// Every provider, in the order Settings lists them.
#[tauri::command]
pub fn credential_providers() -> Vec<ProviderView> {
    ApiProvider::all()
        .map(|provider| ProviderView {
            slug: provider.slug(),
            name: provider.name(),
            env_var: provider.env_var(),
        })
        .collect()
}

#[tauri::command]
pub async fn list_credentials(fleet: FleetState<'_>) -> Result<Vec<CredentialView>> {
    let credentials = desk(&fleet).await?.credentials();
    Ok(credentials.into_iter().map(CredentialView::from).collect())
}

/// Checks and saves one; a refusal comes back as the host's reason.
#[tauri::command]
pub async fn save_credential(
    fleet: FleetState<'_>,
    beep: Service<'_>,
    credential: NewCredential,
) -> Result<CredentialView> {
    let NewCredential {
        kind,
        secret,
        to_machines,
    } = credential;
    save(&fleet, &beep, kind, SecretText::new(secret), to_machines).await
}

/// Checks one with its host, then keeps it for every machine. A GitHub token
/// is saved as the account it signs in as, beside the others. A key that is
/// not for the machines is the server's to keep, so it goes there.
pub(crate) async fn save(
    fleet: &Arc<Fleet>,
    beep: &Beep,
    kind: Kind,
    secret: SecretText,
    to_machines: bool,
) -> Result<CredentialView> {
    let kind = CredentialKind::from(kind);
    match desk(fleet)
        .await?
        .save_credential(&kind, &secret, to_machines)
        .await
    {
        Ok(saved) => Ok(saved.into()),
        Err(FleetError::NeedsServer) => {
            let saved = beep
                .call(SaveCredential {
                    kind,
                    secret,
                    to_machines,
                })
                .await
                .map_err(plain)?;
            Ok(saved.into())
        }
        Err(e) => Err(e.into()),
    }
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
    Ok(desk(&fleet).await?.remove_credential(&id).await?)
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
    Ok(desk(&fleet).await?.set_default_github(&id).await?)
}

/// The server's reason on its own, without the "bad request:" before it.
pub(crate) fn plain(e: DesktopError) -> DesktopError {
    match e {
        DesktopError::Api(ApiError::BadRequest { message }) => DesktopError::BadRequest(message),
        other => other,
    }
}
