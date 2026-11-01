//! The caller's credentials, for the Credentials section of Settings. Every
//! call reads and changes only the caller's own; secrets never come back.

use super::Reply;
use crate::{
    credentials::{Checked, Checker, Slot, last4},
    protocol::{
        ApiError, ApiProvider, Credential, ListCredentials, RemoveCredential, SaveCredential,
    },
    router::Cx,
    store::{CredentialRow, NewCredential},
};
use secrecy::{ExposeSecret, SecretString};
use std::time::SystemTime;

/// The longest secret taken.
const MAX_SECRET: usize = 4096;

pub(super) async fn list_credentials(cx: Cx, _: ListCredentials) -> Reply<Vec<Credential>> {
    Ok(cx
        .user
        .credentials()
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
    let account = check(&slot, &secret).await?;
    let shown = last4(text);
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

/// Asks the host whether `secret` works, and who it signs in as. A Forgejo
/// server on a private address, or a Forgejo server or OpenRouter the server cannot
/// reach, is saved unchecked: a Forgejo server is often on the user's own
/// network, and the server must not probe its own.
async fn check(slot: &Slot, secret: &SecretString) -> Reply<Option<String>> {
    let checked = match slot {
        Slot::GitHub => Checker::new()?.github(secret).await,
        Slot::Forgejo(url) if !url.is_public().await => return Ok(None),
        Slot::Forgejo(url) => Checker::new()?.forgejo(url, secret).await,
        Slot::ApiKey(ApiProvider::OpenRouter) => Checker::new()?.openrouter(secret).await,
        Slot::ApiKey(_) => return Ok(None),
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
        env_var: row.slot.env_var().unwrap_or_default().to_owned(),
        kind: row.slot.to_wire(),
        id: row.id,
        last4: row.last4,
        account: row.account,
        to_machines: row.to_machines,
        created_at: SystemTime::from(row.created_at),
    }
}
