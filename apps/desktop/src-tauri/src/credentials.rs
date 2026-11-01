//! Settings, Credentials: the account's GitHub token, Forgejo servers and
//! API keys. Secrets go to the server once and never come back; the bar
//! sees only their last four characters.

use crate::{
    beep::{Beep, epoch_ms},
    error::{DesktopError, Result},
};
use beep_proto::{
    ApiError, ApiProvider, Credential, CredentialKind, ListCredentials, RemoveCredential,
    SaveCredential, SecretText,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::State;

type Service<'a> = State<'a, Arc<Beep>>;

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
pub async fn list_credentials(beep: Service<'_>) -> Result<Vec<CredentialView>> {
    let credentials = beep.call(ListCredentials {}).await?;
    Ok(credentials.into_iter().map(CredentialView::from).collect())
}

/// Checks and saves one; a refusal comes back as the host's reason.
#[tauri::command]
pub async fn save_credential(
    beep: Service<'_>,
    credential: NewCredential,
) -> Result<CredentialView> {
    let NewCredential {
        kind,
        secret,
        to_machines,
    } = credential;
    let saved = beep
        .call(SaveCredential {
            kind: kind.into(),
            secret: SecretText::new(secret),
            to_machines,
        })
        .await
        .map_err(plain)?;
    Ok(saved.into())
}

#[tauri::command]
pub async fn remove_credential(beep: Service<'_>, id: String) -> Result<()> {
    beep.call(RemoveCredential { id }).await
}

/// The server's reason on its own, without the "bad request:" before it.
fn plain(e: DesktopError) -> DesktopError {
    match e {
        DesktopError::Api(ApiError::BadRequest { message }) => DesktopError::BadRequest(message),
        other => other,
    }
}
