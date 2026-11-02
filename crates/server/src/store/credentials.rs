//! The credentials a user added, sealed (see `crate::credentials`).

use super::Store;
use crate::{
    credentials::{CredentialKey, Opened, Owner, Slot},
    protocol::{ApiProvider, CredentialKind},
    user::UserKey,
};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use secrecy::SecretString;
use sqlx::{PgConnection, Row, postgres::PgRow};
use std::str::FromStr;
use uuid::Uuid;

/// A credential as the apps see it: everything but the secret.
#[derive(Debug, Clone, PartialEq)]
pub struct CredentialRow {
    pub id: String,
    /// As stored, so a row this server does not understand, such as one a
    /// newer server saved, still lists.
    pub kind: CredentialKind,
    pub env_var: String,
    pub last4: String,
    pub account: Option<String>,
    pub to_machines: bool,
    pub created_at: DateTime<Utc>,
    /// Its secret no longer opens, such as after the server's key changed,
    /// or this server does not know its kind. The user has to enter it
    /// again.
    pub unreadable: bool,
}

/// A git host token, opened, with who it signs in as.
#[derive(Debug)]
pub struct GitLogin {
    /// `CredentialRow::id`; a new one each time the token is saved.
    pub id: String,
    pub slot: Slot,
    pub secret: SecretString,
    pub account: Option<String>,
}

/// A credential to save, replacing the user's one in the same slot.
pub struct NewCredential<'a> {
    pub slot: &'a Slot,
    pub secret: &'a SecretString,
    pub last4: &'a str,
    pub account: Option<&'a str>,
    pub to_machines: bool,
}

const GITHUB: &str = "github";
const FORGEJO: &str = "forgejo";
const API_KEY: &str = "api_key";

/// `kind, provider, host_url` of a slot.
struct Columns {
    kind: &'static str,
    provider: &'static str,
    host_url: String,
}

fn columns(slot: &Slot) -> Columns {
    match slot {
        Slot::GitHub => Columns {
            kind: GITHUB,
            provider: "",
            host_url: String::new(),
        },
        Slot::Forgejo(url) => Columns {
            kind: FORGEJO,
            provider: "",
            host_url: url.as_str().to_owned(),
        },
        Slot::ApiKey(provider) => Columns {
            kind: API_KEY,
            provider: provider.slug(),
            host_url: String::new(),
        },
    }
}

fn slot_from(row: &PgRow) -> Result<Slot> {
    let kind: String = row.try_get("kind")?;
    Ok(match kind.as_str() {
        GITHUB => Slot::GitHub,
        FORGEJO => Slot::Forgejo(row.try_get::<String, _>("host_url")?.parse()?),
        API_KEY => {
            let provider: String = row.try_get("provider")?;
            Slot::ApiKey(
                ApiProvider::from_str(&provider)
                    .with_context(|| format!("unknown provider {provider:?}"))?,
            )
        }
        other => bail!("unknown credential kind {other:?}"),
    })
}

/// The row's kind as stored, without checking it.
fn kind_from(row: &PgRow) -> Result<CredentialKind> {
    let kind: String = row.try_get("kind")?;
    Ok(match kind.as_str() {
        GITHUB => CredentialKind::GitHub {},
        FORGEJO => CredentialKind::Forgejo {
            url: row.try_get("host_url")?,
        },
        API_KEY => CredentialKind::ApiKey {
            provider: row.try_get("provider")?,
        },
        _ => CredentialKind::ApiKey { provider: kind },
    })
}

fn credential_from(row: &PgRow, unreadable: bool) -> Result<CredentialRow> {
    Ok(CredentialRow {
        id: row.try_get("id")?,
        kind: kind_from(row)?,
        env_var: row.try_get("env_var")?,
        last4: row.try_get("last4")?,
        account: row.try_get("account")?,
        to_machines: row.try_get("to_machines")?,
        created_at: row.try_get("created_at")?,
        unreadable,
    })
}

const SHOWN: &str =
    "id, kind, provider, host_url, env_var, last4, account, to_machines, created_at";

impl Store {
    /// Seals and saves `new`, replacing the user's credential in its slot.
    pub(crate) async fn save_credential(
        &self,
        user: &UserKey,
        key: &CredentialKey,
        new: NewCredential<'_>,
    ) -> Result<CredentialRow> {
        let id = Uuid::new_v4().simple().to_string();
        let owner = Owner {
            user: user.as_str(),
            slot: new.slot,
            id: &id,
        };
        let sealed = key.seal(&owner, new.secret)?;
        let Columns {
            kind,
            provider,
            host_url,
        } = columns(new.slot);
        let row = sqlx::query(&format!(
            "INSERT INTO credentials (id, user_id, kind, provider, host_url, env_var, to_machines,
                 last4, account, secret)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
             ON CONFLICT (user_id, kind, provider, host_url) DO UPDATE SET id=excluded.id,
                 env_var=excluded.env_var, to_machines=excluded.to_machines,
                 last4=excluded.last4, account=excluded.account, secret=excluded.secret,
                 created_at=now()
             RETURNING {SHOWN}"
        ))
        .bind(&id)
        .bind(user.as_str())
        .bind(kind)
        .bind(provider)
        .bind(host_url)
        .bind(new.slot.env_var().unwrap_or_default())
        .bind(new.to_machines)
        .bind(new.last4)
        .bind(new.account)
        .bind(sealed)
        .fetch_one(&self.pool)
        .await?;
        credential_from(&row, false)
    }

    /// The user's credentials: git hosts first, then API keys. Each is
    /// tried with `key`, to mark the ones that no longer open.
    pub(crate) async fn credentials(
        &self,
        user: &UserKey,
        key: &CredentialKey,
    ) -> Result<Vec<CredentialRow>> {
        let rows = sqlx::query(&format!(
            "SELECT {SHOWN}, secret FROM credentials WHERE user_id=$1
             ORDER BY kind <> 'github', kind <> 'forgejo', host_url, provider"
        ))
        .bind(user.as_str())
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                let id: String = row.try_get("id")?;
                let sealed: Vec<u8> = row.try_get("secret")?;
                let opens = slot_from(row).is_ok_and(|slot| {
                    let owner = Owner {
                        user: user.as_str(),
                        slot: &slot,
                        id: &id,
                    };
                    key.open(&owner, &sealed).is_ok()
                });
                credential_from(row, !opens)
            })
            .collect()
    }

    /// Removes the user's credential `id`; `false` when they have none.
    pub(crate) async fn remove_credential(&self, user: &UserKey, id: &str) -> Result<bool> {
        let done = sqlx::query("DELETE FROM credentials WHERE user_id=$1 AND id=$2")
            .bind(user.as_str())
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(done.rows_affected() > 0)
    }

    /// The user's git host tokens (GitHub, Forgejo), opened, with who each
    /// signs in as. One that does not open is left out and logged, so the
    /// rest still go.
    pub(crate) async fn git_logins(
        &self,
        user: &UserKey,
        key: &CredentialKey,
    ) -> Result<Vec<GitLogin>> {
        let rows = sqlx::query(
            "SELECT id, kind, provider, host_url, account, secret FROM credentials
             WHERE user_id=$1 AND kind IN ('github', 'forgejo') ORDER BY kind, host_url, id",
        )
        .bind(user.as_str())
        .fetch_all(&self.pool)
        .await?;
        let mut out = Vec::new();
        for row in &rows {
            let id: String = row.try_get("id")?;
            let sealed: Vec<u8> = row.try_get("secret")?;
            let account: Option<String> = row.try_get("account")?;
            let opened = slot_from(row).and_then(|slot| {
                let owner = Owner {
                    user: user.as_str(),
                    slot: &slot,
                    id: &id,
                };
                let secret = key.open(&owner, &sealed)?;
                Ok(Opened { slot, secret })
            });
            match opened {
                Ok(Opened { slot, secret }) => out.push(GitLogin {
                    id,
                    slot,
                    secret,
                    account,
                }),
                Err(e) => log::warn!("credential {id} skipped: {e:#}"),
            }
        }
        Ok(out)
    }

    /// The user's credentials meant for their machines, opened. One that
    /// does not open is left out and logged, so the rest still go.
    pub(crate) async fn machine_credentials(
        &self,
        user: &UserKey,
        key: &CredentialKey,
    ) -> Result<Vec<Opened>> {
        let rows = sqlx::query(
            "SELECT id, kind, provider, host_url, secret FROM credentials
             WHERE user_id=$1 AND to_machines ORDER BY kind, host_url, provider",
        )
        .bind(user.as_str())
        .fetch_all(&self.pool)
        .await?;
        let mut out = Vec::new();
        for row in &rows {
            let id: String = row.try_get("id")?;
            let sealed: Vec<u8> = row.try_get("secret")?;
            let opened = slot_from(row).and_then(|slot| {
                let owner = Owner {
                    user: user.as_str(),
                    slot: &slot,
                    id: &id,
                };
                let secret = key.open(&owner, &sealed)?;
                Ok(Opened { slot, secret })
            });
            match opened {
                Ok(opened) => out.push(opened),
                Err(e) => log::warn!("credential {id} skipped: {e:#}"),
            }
        }
        Ok(out)
    }
}

/// Seals the credentials of `source` again for `target`, before an account
/// merge moves them: what is sealed names its user. One that does not open
/// is left as it is, unreadable either way.
pub(super) async fn reseal_for(
    tx: &mut PgConnection,
    key: &CredentialKey,
    source: &str,
    target: &str,
) -> Result<()> {
    let rows = sqlx::query(
        "SELECT id, kind, provider, host_url, secret FROM credentials WHERE user_id=$1",
    )
    .bind(source)
    .fetch_all(&mut *tx)
    .await?;
    for row in &rows {
        let id: String = row.try_get("id")?;
        let sealed: Vec<u8> = row.try_get("secret")?;
        let resealed = slot_from(row).and_then(|slot| {
            let from = Owner {
                user: source,
                slot: &slot,
                id: &id,
            };
            let secret = key.open(&from, &sealed)?;
            let to = Owner {
                user: target,
                slot: &slot,
                id: &id,
            };
            key.seal(&to, &secret)
        });
        match resealed {
            Ok(resealed) => {
                sqlx::query("UPDATE credentials SET secret=$2 WHERE id=$1")
                    .bind(&id)
                    .bind(resealed)
                    .execute(&mut *tx)
                    .await?;
            }
            Err(e) => log::warn!("credential {id} not resealed for the merge: {e:#}"),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::test_db::TestDb;
    use secrecy::ExposeSecret;

    #[tokio::test]
    async fn a_secret_is_sealed_in_its_row_and_opens_back() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = UserKey::for_test("alice");
        let key = CredentialKey::random();
        let secret = SecretString::from("ghp_plaintextvalue9f3c");
        let saved = store
            .save_credential(
                &alice,
                &key,
                NewCredential {
                    slot: &Slot::GitHub,
                    secret: &secret,
                    last4: "9f3c",
                    account: Some("octocat"),
                    to_machines: true,
                },
            )
            .await
            .unwrap();
        assert_eq!(saved.kind, CredentialKind::GitHub {});
        assert_eq!(saved.env_var, "GH_TOKEN");
        assert_eq!(saved.account.as_deref(), Some("octocat"));

        let (sealed, env_var): (Vec<u8>, String) =
            sqlx::query_as("SELECT secret, env_var FROM credentials WHERE id=$1")
                .bind(&saved.id)
                .fetch_one(&store.pool)
                .await
                .unwrap();
        assert_eq!(env_var, "GH_TOKEN");
        assert!(
            !sealed
                .windows(b"plaintext".len())
                .any(|w| w == b"plaintext")
        );
        let opened = store.machine_credentials(&alice, &key).await.unwrap();
        assert_eq!(opened.len(), 1);
        assert_eq!(opened[0].secret.expose_secret(), "ghp_plaintextvalue9f3c");
        assert!(!store.credentials(&alice, &key).await.unwrap()[0].unreadable);
        // Another key opens nothing; the row is skipped, not an error, and
        // listed as unreadable.
        let other = CredentialKey::random();
        assert!(store.credentials(&alice, &other).await.unwrap()[0].unreadable);
        assert!(
            store
                .machine_credentials(&alice, &other)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn credentials_belong_to_their_owner() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = UserKey::for_test("alice");
        let bob = UserKey::for_test("bob");
        let key = CredentialKey::random();
        let forgejo = Slot::Forgejo("https://git.example.com".parse().unwrap());
        let save = |slot: &Slot, secret: &str, to_machines: bool| {
            let secret = SecretString::from(secret);
            let slot = slot.clone();
            let store = store.clone();
            let alice = alice.clone();
            let key = &key;
            async move {
                store
                    .save_credential(
                        &alice,
                        key,
                        NewCredential {
                            slot: &slot,
                            secret: &secret,
                            last4: "0000",
                            account: None,
                            to_machines,
                        },
                    )
                    .await
                    .unwrap()
            }
        };
        let first = save(&forgejo, "token-one", true).await;
        let second = save(
            &Slot::Forgejo("https://code.example.org".parse().unwrap()),
            "token-two",
            true,
        )
        .await;
        save(&Slot::ApiKey(ApiProvider::OpenAi), "sk-kept-here", false).await;
        // Saving the same server again replaces it.
        let replaced = save(&forgejo, "token-three", true).await;
        assert_ne!(replaced.id, first.id);

        assert!(store.credentials(&bob, &key).await.unwrap().is_empty());
        assert!(!store.remove_credential(&bob, &second.id).await.unwrap());
        assert!(
            store
                .machine_credentials(&bob, &key)
                .await
                .unwrap()
                .is_empty()
        );

        let listed = store.credentials(&alice, &key).await.unwrap();
        assert_eq!(listed.len(), 3);
        // Only those marked for machines go to them.
        let opened = store.machine_credentials(&alice, &key).await.unwrap();
        let secrets: Vec<&str> = opened.iter().map(|o| o.secret.expose_secret()).collect();
        assert_eq!(secrets, ["token-two", "token-three"]);
        // The git hosts open for the overview, API keys never.
        let git = store.git_logins(&alice, &key).await.unwrap();
        let hosts: Vec<(&str, &str)> = git
            .iter()
            .map(|login| {
                let host = match &login.slot {
                    Slot::Forgejo(url) => url.as_str(),
                    _ => "not a forgejo server",
                };
                (host, login.secret.expose_secret())
            })
            .collect();
        assert_eq!(
            hosts,
            [
                ("https://code.example.org", "token-two"),
                ("https://git.example.com", "token-three")
            ]
        );
        assert!(store.git_logins(&bob, &key).await.unwrap().is_empty());

        assert!(store.remove_credential(&alice, &second.id).await.unwrap());
        assert_eq!(store.credentials(&alice, &key).await.unwrap().len(), 2);

        // A row moved to another user, as by a write that skipped the
        // store, does not open for them.
        sqlx::query("UPDATE credentials SET user_id=$1 WHERE id=$2")
            .bind(bob.as_str())
            .bind(&replaced.id)
            .execute(&store.pool)
            .await
            .unwrap();
        let moved = store.credentials(&bob, &key).await.unwrap();
        assert_eq!(moved.len(), 1);
        assert!(moved[0].unreadable);

        // A provider this server does not know, as a newer one saved it,
        // lists as unreadable without failing the rest.
        sqlx::query("UPDATE credentials SET provider='newai' WHERE user_id=$1 AND kind='api_key'")
            .bind(alice.as_str())
            .execute(&store.pool)
            .await
            .unwrap();
        let listed = store.credentials(&alice, &key).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(
            listed[0].kind,
            CredentialKind::ApiKey {
                provider: "newai".into()
            }
        );
        assert!(listed[0].unreadable);
        assert!(
            store
                .machine_credentials(&bob, &key)
                .await
                .unwrap()
                .is_empty()
        );
    }
}
