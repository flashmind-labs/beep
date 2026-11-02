//! Join codes, and moving one account into another when a device joins.
//!
//! A device that already has an account joins another by its code: every
//! row of its own account moves to the other, its keys become the other's
//! linked keys, and its account goes. Where both accounts hold the same
//! thing, such as a place called Home or a setting, the account joined
//! keeps its own.

use super::{Account, Bumped, NewKey, Store, credentials::reseal_for, groups::new_token};
use crate::{credentials::CredentialKey, protocol::DeviceKind, user::UserKey};
use anyhow::Result;
use chrono::{DateTime, Utc};
use iroh::EndpointId;
use sqlx::PgConnection;
use std::time::Duration;

/// A code that lets another device join the account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invite {
    pub code: String,
    pub expires_at: DateTime<Utc>,
}

/// The account and key behind a live code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inviter {
    pub account: Account,
    pub name: String,
    pub kind: DeviceKind,
}

/// How a join went.
#[derive(Debug, PartialEq, Eq)]
pub enum Merge {
    /// The joining account is gone; its keys now sign into `target`.
    Merged { target: String, keys: Vec<String> },
    /// The code is unknown, used or expired, or names another account than
    /// the one the caller looked up.
    NoInvite,
    /// The code is the caller's own account's.
    SameAccount,
    /// One of the joining account's chats has a run.
    Busy,
}

/// What moves an account into another.
pub struct MergeInto<'a> {
    pub code: &'a str,
    /// The account the caller looked the code up as.
    pub target: &'a str,
    /// Whether the account joined takes the joining one's outside id, so
    /// the apps connected under it stay connected.
    pub adopt_external_id: bool,
    /// What the credentials are sealed with, to seal them again for the
    /// account joined.
    pub credential_key: &'a CredentialKey,
}

/// Tables whose rows simply change owner. Where both accounts have a row
/// with the same `key` columns, the joined account's row stays and the
/// other goes.
const MOVED: &[(&str, &[&str])] = &[
    ("runs", &["request_id"]),
    ("files", &[]),
    ("route_cards", &["call_id"]),
    ("shares", &[]),
    ("actions", &[]),
    ("profile_notes", &[]),
    ("devices", &["device_id"]),
    ("mail_sync", &["mailbox"]),
    ("mail_seen", &["mailbox", "message_id"]),
    ("attention_items", &["mailbox", "message_id"]),
    ("profile_reads", &["source"]),
    ("memory_curation", &["scope"]),
    ("app_permissions", &["app"]),
    ("mcp_servers", &["name"]),
    ("app_events", &["dedupe_key"]),
    ("event_triage", &[]),
    ("event_topics", &["topic"]),
    ("event_triggers", &["slug", "account_id"]),
    ("event_polls", &["source"]),
    ("nodes", &[]),
    ("project_sync", &["project", "endpoint_id"]),
    ("pinned_projects", &["project"]),
    ("credentials", &["kind", "provider", "host_url"]),
];

impl Store {
    /// A new code for the account of `user`, shown by `endpoint`, which
    /// takes `name`. Earlier codes of that key stop working.
    pub(crate) async fn create_invite(
        &self,
        user: &UserKey,
        endpoint: &EndpointId,
        name: &str,
        ttl: Duration,
    ) -> Result<Invite> {
        let endpoint = endpoint.to_string();
        let expires_at = Utc::now() + ttl;
        let mut tx = self.pool.begin().await?;
        sqlx::query("UPDATE user_keys SET name=$3 WHERE user_id=$1 AND endpoint_id=$2")
            .bind(user.as_str())
            .bind(&endpoint)
            .bind(name)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM account_invites WHERE endpoint_id=$1 OR expires_at < now()")
            .bind(&endpoint)
            .execute(&mut *tx)
            .await?;
        let code = new_token();
        sqlx::query(
            "INSERT INTO account_invites (code, user_id, endpoint_id, expires_at)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(&code)
        .bind(user.as_str())
        .bind(&endpoint)
        .bind(expires_at)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Invite { code, expires_at })
    }

    /// Who made the live code `code`.
    pub async fn peek_invite(&self, code: &str) -> Result<Option<Inviter>> {
        let row: Option<(String, String, String, String)> = sqlx::query_as(
            "SELECT u.id, u.external_id, k.name, k.kind
             FROM account_invites i
             JOIN users u ON u.id = i.user_id
             JOIN user_keys k ON k.endpoint_id = i.endpoint_id AND k.user_id = i.user_id
             WHERE i.code=$1 AND i.expires_at > now() AND k.revoked_at IS NULL",
        )
        .bind(code)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(id, external_id, name, kind)| Inviter {
            account: Account { id, external_id },
            name,
            kind: DeviceKind::parse(&kind).unwrap_or_default(),
        }))
    }

    /// Links `key`, which has no account, to the one that made the live
    /// code `code`, using the code up. `None` when the code is not live or
    /// the key is linked already.
    pub async fn redeem_invite(&self, code: &str, key: NewKey<'_>) -> Result<Option<String>> {
        let mut tx = self.pool.begin().await?;
        let target: Option<String> = sqlx::query_scalar(
            "DELETE FROM account_invites WHERE code=$1 AND expires_at > now() RETURNING user_id",
        )
        .bind(code)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(target) = target else {
            tx.rollback().await?;
            return Ok(None);
        };
        let linked = sqlx::query(
            "INSERT INTO user_keys (endpoint_id, user_id, name, kind) VALUES ($1, $2, $3, $4)
             ON CONFLICT (endpoint_id) DO NOTHING",
        )
        .bind(key.endpoint.to_string())
        .bind(&target)
        .bind(key.name)
        .bind(key.kind.as_str())
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if linked == 0 {
            // The code stays usable for the key it was meant for.
            tx.rollback().await?;
            return Ok(None);
        }
        tx.commit().await?;
        Ok(Some(target))
    }

    /// Moves the account of `source` into the one that made the code,
    /// using the code up.
    pub(crate) async fn merge_into(&self, source: &UserKey, into: MergeInto<'_>) -> Result<Merge> {
        let source = source.as_str();
        let mut tx = self.pool.begin().await?;
        let target: Option<String> = sqlx::query_scalar(
            "SELECT user_id FROM account_invites WHERE code=$1 AND expires_at > now() FOR UPDATE",
        )
        .bind(into.code)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(target) = target.filter(|target| target == into.target) else {
            tx.rollback().await?;
            return Ok(Merge::NoInvite);
        };
        if target == source {
            tx.rollback().await?;
            return Ok(Merge::SameAccount);
        }
        // Both accounts' rows, in id order so two joins cannot deadlock.
        sqlx::query("SELECT id FROM users WHERE id = ANY($1) ORDER BY id FOR UPDATE")
            .bind(vec![source, target.as_str()])
            .execute(&mut *tx)
            .await?;
        let busy: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM runs WHERE user_id=$1 AND status='running')",
        )
        .bind(source)
        .fetch_one(&mut *tx)
        .await?;
        if busy {
            tx.rollback().await?;
            return Ok(Merge::Busy);
        }
        sqlx::query("DELETE FROM account_invites WHERE code=$1")
            .bind(into.code)
            .execute(&mut *tx)
            .await?;

        move_chats(&mut tx, source, &target).await?;
        let bumped = move_updates(&mut tx, source, &target).await?;
        reseal_for(&mut tx, into.credential_key, source, &target).await?;
        // The joined account keeps its default GitHub account.
        sqlx::query(
            "UPDATE credentials SET is_default=false WHERE user_id=$1 AND kind='github'
             AND EXISTS (SELECT 1 FROM credentials WHERE user_id=$2 AND kind='github')",
        )
        .bind(source)
        .bind(&target)
        .execute(&mut *tx)
        .await?;
        for (table, key) in MOVED {
            move_rows(&mut tx, table, key, source, &target).await?;
        }
        move_settings(&mut tx, source, &target).await?;
        move_places(&mut tx, source, &target).await?;

        // Memories carry their owner in their metadata, and earlier
        // versions of them in `history_of`.
        sqlx::query(
            "UPDATE memory_meta SET value=$2 WHERE key IN ('user', 'history_of') AND value=$1",
        )
        .bind(source)
        .bind(&target)
        .execute(&mut *tx)
        .await?;
        let keys: Vec<String> = sqlx::query_scalar(
            "UPDATE user_keys SET user_id=$2, is_primary=false WHERE user_id=$1
             RETURNING endpoint_id",
        )
        .bind(source)
        .bind(&target)
        .fetch_all(&mut *tx)
        .await?;
        if into.adopt_external_id {
            sqlx::query("UPDATE users SET external_id = s.external_id FROM users s WHERE users.id=$2 AND s.id=$1")
                .bind(source)
                .bind(&target)
                .execute(&mut *tx)
                .await?;
        }
        // Whatever is left, such as its sync counter, goes with the account.
        sqlx::query("DELETE FROM users WHERE id=$1")
            .bind(source)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        self.announce(&[bumped]);
        self.notify_crons_changed();
        Ok(Merge::Merged { target, keys })
    }

    /// The account with id `id`.
    pub async fn account(&self, id: &str) -> Result<Option<Account>> {
        let row: Option<(String, String)> =
            sqlx::query_as("SELECT id, external_id FROM users WHERE id=$1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|(id, external_id)| Account { id, external_id }))
    }
}

/// Moves the source's chat memberships, keeping the target's where both
/// are in one group, and the messages the source wrote.
async fn move_chats(tx: &mut PgConnection, source: &str, target: &str) -> Result<()> {
    sqlx::query(
        "DELETE FROM chat_members s WHERE s.user_id=$1
         AND EXISTS (SELECT 1 FROM chat_members t WHERE t.user_id=$2 AND t.chat_id=s.chat_id)",
    )
    .bind(source)
    .bind(target)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE chat_members SET user_id=$2 WHERE user_id=$1")
        .bind(source)
        .bind(target)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE messages SET author_id=$2 WHERE author_id=$1")
        .bind(source)
        .bind(target)
        .execute(&mut *tx)
        .await?;
    Ok(())
}

/// Appends the source's update log to the target's, in order, and makes
/// every client of either start over, since each holds only half.
async fn move_updates(tx: &mut PgConnection, source: &str, target: &str) -> Result<Bumped> {
    sqlx::query(
        "DELETE FROM updates s WHERE s.user_id=$1 AND EXISTS (
             SELECT 1 FROM updates t WHERE t.user_id=$2 AND t.chat_id=s.chat_id
             AND t.message_id IS NOT DISTINCT FROM s.message_id)",
    )
    .bind(source)
    .bind(target)
    .execute(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO sync_counters (user_id) VALUES ($1), ($2) ON CONFLICT DO NOTHING")
        .bind(source)
        .bind(target)
        .execute(&mut *tx)
        .await?;
    let offset: i64 = sqlx::query_scalar("SELECT last FROM sync_counters WHERE user_id=$1")
        .bind(target)
        .fetch_one(&mut *tx)
        .await?;
    sqlx::query("UPDATE updates SET user_id=$2, update_id = update_id + $3 WHERE user_id=$1")
        .bind(source)
        .bind(target)
        .bind(offset)
        .execute(&mut *tx)
        .await?;
    // `updates_page` resets a client whose last id is below
    // `pruned_below - 1`, which is every client either account had.
    let last: i64 = sqlx::query_scalar(
        "UPDATE sync_counters t SET last = t.last + s.last,
             pruned_below = GREATEST(t.pruned_below, t.last + s.last + 1)
         FROM sync_counters s WHERE t.user_id=$2 AND s.user_id=$1
         RETURNING t.last",
    )
    .bind(source)
    .bind(target)
    .fetch_one(&mut *tx)
    .await?;
    Ok(Bumped {
        user_id: target.to_owned(),
        last,
    })
}

/// Moves the rows of `table`, dropping the source's where the target has
/// one with the same `key`.
async fn move_rows(
    tx: &mut PgConnection,
    table: &str,
    key: &[&str],
    source: &str,
    target: &str,
) -> Result<()> {
    if !key.is_empty() {
        let same: String = key.iter().map(|c| format!(" AND t.{c}=s.{c}")).collect();
        sqlx::query(&format!(
            "DELETE FROM {table} s WHERE s.user_id=$1
             AND EXISTS (SELECT 1 FROM {table} t WHERE t.user_id=$2{same})"
        ))
        .bind(source)
        .bind(target)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query(&format!("UPDATE {table} SET user_id=$2 WHERE user_id=$1"))
        .bind(source)
        .bind(target)
        .execute(&mut *tx)
        .await?;
    Ok(())
}

/// Habits, skills, token use and the last location, which each merge in
/// their own way.
async fn move_settings(tx: &mut PgConnection, source: &str, target: &str) -> Result<()> {
    // A habit both track keeps the target's, with the source's logs.
    sqlx::query(
        "UPDATE habit_logs l SET habit_id = t.id FROM habits s, habits t
         WHERE l.habit_id = s.id AND s.user_id=$1 AND t.user_id=$2 AND t.name = s.name",
    )
    .bind(source)
    .bind(target)
    .execute(&mut *tx)
    .await?;
    move_rows(tx, "habits", &["name"], source, target).await?;
    move_rows(tx, "habit_logs", &[], source, target).await?;

    sqlx::query(
        "DELETE FROM skills s WHERE s.user_id=$1 AND EXISTS (
             SELECT 1 FROM skills t WHERE t.user_id=$2 AND lower(t.name) = lower(s.name))",
    )
    .bind(source)
    .bind(target)
    .execute(&mut *tx)
    .await?;
    move_rows(tx, "skills", &[], source, target).await?;

    sqlx::query(
        "INSERT INTO token_usage (user_id, day, model, agent, calls, prompt_tokens,
             cached_tokens, cache_write_tokens, completion_tokens, cost)
         SELECT $2, day, model, agent, calls, prompt_tokens, cached_tokens, cache_write_tokens,
             completion_tokens, cost
         FROM token_usage WHERE user_id=$1
         ON CONFLICT (user_id, day, model, agent) DO UPDATE SET
             calls = token_usage.calls + excluded.calls,
             prompt_tokens = token_usage.prompt_tokens + excluded.prompt_tokens,
             cached_tokens = token_usage.cached_tokens + excluded.cached_tokens,
             cache_write_tokens = token_usage.cache_write_tokens + excluded.cache_write_tokens,
             completion_tokens = token_usage.completion_tokens + excluded.completion_tokens,
             cost = token_usage.cost + excluded.cost",
    )
    .bind(source)
    .bind(target)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM token_usage WHERE user_id=$1")
        .bind(source)
        .execute(&mut *tx)
        .await?;

    // The newer fix wins, which is usually the phone's.
    sqlx::query(
        "DELETE FROM locations l WHERE l.user_id = ANY($1) AND EXISTS (
             SELECT 1 FROM locations o WHERE o.user_id = ANY($1) AND o.user_id <> l.user_id
             AND (o.updated_at > l.updated_at OR (o.updated_at = l.updated_at AND o.user_id = $2)))",
    )
    .bind(vec![source, target])
    .bind(target)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE locations SET user_id=$2 WHERE user_id=$1")
        .bind(source)
        .bind(target)
        .execute(&mut *tx)
        .await?;
    // A timezone the target never set comes from the source.
    sqlx::query(
        "UPDATE users t SET timezone = s.timezone FROM users s
         WHERE t.id=$2 AND s.id=$1 AND t.timezone IS NULL",
    )
    .bind(source)
    .bind(target)
    .execute(&mut *tx)
    .await?;
    Ok(())
}

/// Moves places and routines. A place both saved, by name, keeps the
/// target's, and the source's routines there point at it.
async fn move_places(tx: &mut PgConnection, source: &str, target: &str) -> Result<()> {
    // Copies first: a routine's place is a foreign key, so the place it
    // moves to must exist when the routine moves.
    sqlx::query(
        "INSERT INTO places (user_id, id, name, latitude, longitude, radius, address,
             created_at, inside, last_here_at)
         SELECT $2, s.id, s.name, s.latitude, s.longitude, s.radius, s.address, s.created_at,
             s.inside, s.last_here_at
         FROM places s WHERE s.user_id=$1 AND NOT EXISTS (
             SELECT 1 FROM places t WHERE t.user_id=$2 AND lower(t.name) = lower(s.name))",
    )
    .bind(source)
    .bind(target)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        r"UPDATE crons c SET user_id=$2, schedule = COALESCE((
             SELECT regexp_replace(c.schedule, '^(arrive|leave):[^:]+', '\1:' || t.id)
             FROM places s JOIN places t ON t.user_id=$2 AND lower(t.name) = lower(s.name)
             WHERE s.user_id=$1 AND s.id = c.place_id AND t.id <> s.id), c.schedule)
         WHERE c.user_id=$1",
    )
    .bind(source)
    .bind(target)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM places WHERE user_id=$1")
        .bind(source)
        .execute(&mut *tx)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{NewGitHubAccount, SignIn, TestDb, test_chat, test_turn};
    use iroh::SecretKey;
    use secrecy::{ExposeSecret, SecretString};

    fn key(seed: u8) -> EndpointId {
        SecretKey::from_bytes(&[seed; 32]).public()
    }

    async fn account(store: &Store, endpoint: &EndpointId) -> String {
        match store.sign_in(endpoint).await.unwrap() {
            SignIn::Account { account, .. } => account.id,
            SignIn::Revoked => panic!("a new key makes an account"),
        }
    }

    #[tokio::test]
    async fn a_phone_joins_a_computers_account_with_its_data() {
        let db = TestDb::new().await.expect("test database");
        let store = db.store().await.expect("test store");
        let credential_key = CredentialKey::random();
        let (mac, phone) = (key(11), key(12));
        let target = account(&store, &mac).await;
        let source = account(&store, &phone).await;
        let (target_key, source_key) = (UserKey::for_test(&target), UserKey::for_test(&source));
        let pool = &store.pool;

        // Both saved Home; only the phone saved Gym, with a routine at each.
        for (user, id, name) in [
            (&target, "t-home", "Home"),
            (&source, "s-home", "home"),
            (&source, "s-gym", "Gym"),
        ] {
            sqlx::query(
                "INSERT INTO places (user_id, id, name, latitude, longitude, radius)
                 VALUES ($1, $2, $3, 1, 1, 100)",
            )
            .bind(user)
            .bind(id)
            .bind(name)
            .execute(pool)
            .await
            .unwrap();
        }
        for (id, schedule) in [
            ("c1", "arrive:s-home"),
            ("c2", "leave:s-gym:once"),
            ("c3", "0 9 * * *"),
        ] {
            sqlx::query(
                "INSERT INTO crons (user_id, id, label, prompt, schedule) VALUES ($1, $2, '', '', $3)",
            )
            .bind(&source)
            .bind(id)
            .bind(schedule)
            .execute(pool)
            .await
            .unwrap();
        }
        let chat = test_chat("phone chat");
        test_turn(&store, &source_key, chat, "from the phone", 1000)
            .await
            .unwrap();
        test_turn(
            &store,
            &target_key,
            test_chat("mac chat"),
            "from the mac",
            2000,
        )
        .await
        .unwrap();

        let invite = store
            .create_invite(&target_key, &mac, "MacBook", Duration::from_secs(600))
            .await
            .unwrap();
        let inviter = store.peek_invite(&invite.code).await.unwrap().unwrap();
        assert_eq!(inviter.account.id, target);
        assert_eq!(inviter.name, "MacBook");

        let into = MergeInto {
            code: &invite.code,
            target: &target,
            adopt_external_id: false,
            credential_key: &credential_key,
        };
        assert_eq!(
            store.merge_into(&target_key, into).await.unwrap(),
            Merge::SameAccount
        );
        let into = MergeInto {
            code: &invite.code,
            target: &target,
            adopt_external_id: true,
            credential_key: &credential_key,
        };
        let Merge::Merged {
            target: merged,
            keys,
        } = store.merge_into(&source_key, into).await.unwrap()
        else {
            panic!("the phone joins");
        };
        assert_eq!(merged, target);
        assert_eq!(keys, vec![phone.to_string()]);

        // The phone signs into the computer's account, as a linked key.
        assert_eq!(
            store.sign_in(&phone).await.unwrap(),
            SignIn::Account {
                account: Account {
                    id: target.clone(),
                    external_id: source.clone(),
                },
                device: DeviceKind::Phone {},
                primary: false,
            }
        );
        assert!(store.account(&source).await.unwrap().is_none());

        // The chat moved, and the target's log holds it.
        let members: Vec<String> =
            sqlx::query_scalar("SELECT user_id FROM chat_members WHERE chat_id=$1")
                .bind(chat)
                .fetch_all(pool)
                .await
                .unwrap();
        assert_eq!(members, vec![target.clone()]);
        let logged: i64 =
            sqlx::query_scalar("SELECT count(*) FROM updates WHERE user_id=$1 AND chat_id=$2")
                .bind(&target)
                .bind(chat)
                .fetch_one(pool)
                .await
                .unwrap();
        assert!(logged > 0);
        let page = store.updates_page(&target_key, 1).await.unwrap();
        assert!(page.reset, "clients of either account start over");
        assert_eq!(page.updates.len(), 6, "both chats, each with a turn");
        let again = store.updates_page(&target_key, page.last).await.unwrap();
        assert!(!again.reset && again.updates.is_empty());

        // Routines point at the target's Home and the moved Gym.
        let schedules: Vec<String> =
            sqlx::query_scalar("SELECT schedule FROM crons WHERE user_id=$1 ORDER BY id")
                .bind(&target)
                .fetch_all(pool)
                .await
                .unwrap();
        assert_eq!(
            schedules,
            ["arrive:t-home", "leave:s-gym:once", "0 9 * * *"]
        );
        let places: Vec<String> =
            sqlx::query_scalar("SELECT id FROM places WHERE user_id=$1 ORDER BY id")
                .bind(&target)
                .fetch_all(pool)
                .await
                .unwrap();
        assert_eq!(places, ["s-gym", "t-home"]);

        // The code works once.
        let into = MergeInto {
            code: &invite.code,
            target: &target,
            adopt_external_id: false,
            credential_key: &credential_key,
        };
        assert_eq!(
            store.merge_into(&source_key, into).await.unwrap(),
            Merge::NoInvite
        );
    }

    #[tokio::test]
    async fn events_and_machines_move_with_the_account() {
        let db = TestDb::new().await.expect("test database");
        let store = db.store().await.expect("test store");
        let credential_key = CredentialKey::random();
        let (mac, phone, node) = (key(21), key(22), key(23));
        let target = account(&store, &mac).await;
        let source = account(&store, &phone).await;
        let target_key = UserKey::for_test(&target);
        let source_key = UserKey::for_test(&source);
        let pool = &store.pool;
        for (user, dedupe) in [(&source, "a"), (&source, "b"), (&target, "a")] {
            sqlx::query(
                "INSERT INTO app_events (user_id, app, kind, dedupe_key, summary, ids, raw, source, occurred_at)
                 VALUES ($1, 'gmail', 'new_mail', $2, '', '{}', '{}', 'poll', now())",
            )
            .bind(user)
            .bind(dedupe)
            .execute(pool)
            .await
            .unwrap();
        }
        for query in [
            "INSERT INTO event_triage (user_id, decision, reason) VALUES ($1, 'ignore', '')",
            "INSERT INTO event_topics (user_id, topic, enabled) VALUES ($1, 'github_ci', false)",
            "INSERT INTO event_triggers (user_id, slug, account_id, trigger_id) VALUES ($1, 's', 'a', 't')",
            "INSERT INTO event_polls (user_id, source, polled_at) VALUES ($1, 'github', now())",
        ] {
            sqlx::query(query)
                .bind(&source)
                .execute(pool)
                .await
                .unwrap();
        }
        store
            .node_connected(
                &source_key,
                crate::store::NewNode {
                    endpoint: node,
                    name: "build box",
                    version: "0.1.0",
                },
            )
            .await
            .unwrap();

        let token = SecretString::from("ghp_moved0000");
        store
            .save_github_account(
                &source_key,
                &credential_key,
                NewGitHubAccount {
                    secret: &token,
                    last4: "0000",
                    login: "octo-one",
                    orgs: &[],
                    replace_default: false,
                },
            )
            .await
            .unwrap();

        // Both have a default GitHub account; the joined one keeps its own.
        let kept = SecretString::from("ghp_kept00001111");
        store
            .save_github_account(
                &target_key,
                &credential_key,
                NewGitHubAccount {
                    secret: &kept,
                    last4: "1111",
                    login: "octo-two",
                    orgs: &[],
                    replace_default: false,
                },
            )
            .await
            .unwrap();

        let invite = store
            .create_invite(&target_key, &mac, "MacBook", Duration::from_secs(600))
            .await
            .unwrap();
        let into = MergeInto {
            code: &invite.code,
            target: &target,
            adopt_external_id: false,
            credential_key: &credential_key,
        };
        assert!(matches!(
            store.merge_into(&source_key, into).await.unwrap(),
            Merge::Merged { .. }
        ));

        for (table, rows) in [
            ("app_events", 2),
            ("event_triage", 1),
            ("event_topics", 1),
            ("event_triggers", 1),
            ("event_polls", 1),
            ("nodes", 1),
            ("credentials", 2),
        ] {
            let query = format!("SELECT count(*) FROM {table} WHERE user_id=$1");
            for (user, expected) in [(&source, 0), (&target, rows)] {
                let count: i64 = sqlx::query_scalar(&query)
                    .bind(user)
                    .fetch_one(pool)
                    .await
                    .unwrap();
                assert_eq!(count, expected, "{table} for {user}");
            }
        }
        // The GitHub token moved sealed for the account joined, and opens.
        let listed = store
            .credentials(&target_key, &credential_key)
            .await
            .unwrap();
        assert!(!listed[0].unreadable);
        let accounts = store
            .github_accounts(&target_key, &credential_key)
            .await
            .unwrap();
        let logins: Vec<(&str, bool)> = accounts
            .iter()
            .map(|a| (a.login.as_str(), a.default))
            .collect();
        assert_eq!(logins, [("octo-two", true), ("octo-one", false)]);
        let opened = store
            .machine_credentials(&target_key, &credential_key)
            .await
            .unwrap();
        assert_eq!(opened[1].secret.expose_secret(), "ghp_moved0000");
    }
}
