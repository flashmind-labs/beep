//! The credentials a user added, sealed (see `crate::credentials`).

use super::Store;
use crate::{
    credentials::{CredentialKey, Opened, Slot},
    protocol::ApiProvider,
    user::UserKey,
};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use secrecy::SecretString;
use sqlx::{Row, postgres::PgRow};
use std::str::FromStr;
use uuid::Uuid;

/// A credential as the apps see it: everything but the secret.
#[derive(Debug, Clone, PartialEq)]
pub struct CredentialRow {
    pub id: String,
    pub slot: Slot,
    pub last4: String,
    pub account: Option<String>,
    pub to_machines: bool,
    pub created_at: DateTime<Utc>,
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

fn credential_from(row: &PgRow) -> Result<CredentialRow> {
    Ok(CredentialRow {
        id: row.try_get("id")?,
        slot: slot_from(row)?,
        last4: row.try_get("last4")?,
        account: row.try_get("account")?,
        to_machines: row.try_get("to_machines")?,
        created_at: row.try_get("created_at")?,
    })
}

const SHOWN: &str = "id, kind, provider, host_url, last4, account, to_machines, created_at";

impl Store {
    /// Seals and saves `new`, replacing the user's credential in its slot.
    pub(crate) async fn save_credential(
        &self,
        user: &UserKey,
        key: &CredentialKey,
        new: NewCredential<'_>,
    ) -> Result<CredentialRow> {
        let id = Uuid::new_v4().simple().to_string();
        let sealed = key.seal(&id, new.secret)?;
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
        credential_from(&row)
    }

    /// The user's credentials: git hosts first, then API keys.
    pub(crate) async fn credentials(&self, user: &UserKey) -> Result<Vec<CredentialRow>> {
        let rows = sqlx::query(&format!(
            "SELECT {SHOWN} FROM credentials WHERE user_id=$1
             ORDER BY kind <> 'github', kind <> 'forgejo', host_url, provider"
        ))
        .bind(user.as_str())
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(credential_from).collect()
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
            match (slot_from(row), key.open(&id, &sealed)) {
                (Ok(slot), Ok(secret)) => out.push(Opened { slot, secret }),
                (Err(e), _) | (_, Err(e)) => log::warn!("credential {id} skipped: {e:#}"),
            }
        }
        Ok(out)
    }
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
        assert_eq!(saved.slot, Slot::GitHub);
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
        // Another key opens nothing; the row is skipped, not an error.
        let other = CredentialKey::random();
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

        assert!(store.credentials(&bob).await.unwrap().is_empty());
        assert!(!store.remove_credential(&bob, &second.id).await.unwrap());
        assert!(
            store
                .machine_credentials(&bob, &key)
                .await
                .unwrap()
                .is_empty()
        );

        let listed = store.credentials(&alice).await.unwrap();
        assert_eq!(listed.len(), 3);
        // Only those marked for machines go to them.
        let opened = store.machine_credentials(&alice, &key).await.unwrap();
        let secrets: Vec<&str> = opened.iter().map(|o| o.secret.expose_secret()).collect();
        assert_eq!(secrets, ["token-two", "token-three"]);

        assert!(store.remove_credential(&alice, &second.id).await.unwrap());
        assert_eq!(store.credentials(&alice).await.unwrap().len(), 2);
    }
}
