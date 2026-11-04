//! A throwaway database on the test server, so tests run in parallel
//! without seeing each other's rows.

use super::{
    CreateRun, FinishedTurn, RunStatus, Store, WordEmbedder,
    messages::{Step, TurnRecord},
};
use crate::{account::user::UserKey, agent::steps};
use anyhow::{Context, Result, bail};
use chrono::DateTime;
use flashmind::{
    core::{Conversation, ConversationEntry},
    memory::EmbeddingProvider,
    types::ToolCall,
};
use serde_json::json;
use sqlx::{
    Connection, Executor, PgConnection, PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use std::{
    collections::HashMap,
    str::FromStr,
    sync::{Arc, mpsc},
    thread,
};
use uuid::Uuid;

/// Names the database tests run against, such as
/// `postgres://beep:beep@127.0.0.1:55432/beep`.
const URL_VAR: &str = "BEEP_TEST_DATABASE_URL";

/// Accounts every test store has, since rows need an account to belong to.
const TEST_USERS: &[&str] = &["alice", "bob", "user"];

/// The same id for the same `name`, so a test can name its chats.
#[doc(hidden)]
pub fn test_chat(name: &str) -> Uuid {
    let mut hash: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
    for byte in name.bytes() {
        hash ^= u128::from(byte);
        hash = hash.wrapping_mul(0x0000_0000_0100_0000_0000_0000_0000_013b);
    }
    Uuid::from_u128(hash)
}

/// What the apps show of `conversation`, as a finished run saves it.
#[doc(hidden)]
pub fn test_steps(conversation: &Conversation) -> Vec<Step> {
    steps::steps(
        conversation.entries(),
        None,
        &steps::CallNotes::default(),
        &HashMap::new(),
    )
    .steps
}

/// Runs one turn in session `chat` of `user`: `prompt` sent at `at`
/// milliseconds and a reply a millisecond later. Returns the id of the
/// message that opened it.
#[doc(hidden)]
pub async fn test_turn(
    store: &Store,
    user: &UserKey,
    chat: Uuid,
    prompt: &str,
    at: i64,
) -> Result<i64> {
    let turn = TestTurn {
        prompt,
        at,
        calls: 0,
    };
    test_turn_with(store, user, chat, turn).await
}

/// A turn for [`test_turn_with`].
#[doc(hidden)]
pub struct TestTurn<'a> {
    pub prompt: &'a str,
    /// When it was sent, in milliseconds.
    pub at: i64,
    /// Tool calls Beep makes before replying, each one succeeding.
    pub calls: usize,
}

/// [`test_turn`] with tool calls between the prompt and the reply, each a
/// millisecond apart.
#[doc(hidden)]
pub async fn test_turn_with(
    store: &Store,
    user: &UserKey,
    chat: Uuid,
    turn: TestTurn<'_>,
) -> Result<i64> {
    let TestTurn { prompt, at, calls } = turn;
    let CreateRun::Created(run) = store
        .create_run(user, chat, &format!("req-{chat}-{at}"), prompt)
        .await?
    else {
        bail!("the run in {chat} was not created");
    };
    let mut turn = Conversation::new();
    turn.add(ConversationEntry::user(prompt));
    if calls > 0 {
        let ids: Vec<String> = (0..calls).map(|i| format!("call-{at}-{i}")).collect();
        let tool_calls = ids
            .iter()
            .map(|id| ToolCall {
                id: id.clone(),
                name: "web_search".to_owned(),
                arguments: json!({}),
            })
            .collect();
        turn.add(ConversationEntry::assistant_with_tool_calls(
            "", tool_calls, None,
        ));
        for id in &ids {
            turn.add(ConversationEntry::tool(id, "ok"));
        }
    }
    turn.add(ConversationEntry::assistant(format!("re: {prompt}")));
    for (i, entry) in turn.entries_mut().iter_mut().enumerate() {
        entry.timestamp =
            DateTime::from_timestamp_millis(at + i as i64).context("time out of range")?;
    }
    let mut context = match store.context(user, chat).await? {
        Some(stored) => stored.conversation,
        None => Conversation::new(),
    };
    context.entries_mut().extend(turn.entries().iter().cloned());
    let steps = test_steps(&turn);
    let saved = store
        .finish(
            user,
            &run,
            Some(FinishedTurn {
                context: &context,
                prompt_tokens: Some(0),
                steps: &steps,
                stands_alone: false,
                carried: TurnRecord::default(),
            }),
            RunStatus::Completed,
        )
        .await?;
    saved
        .and_then(|saved| saved.opened)
        .context("the turn opened no message")
}

/// A database of its own on the test server, dropped with this value.
#[doc(hidden)]
pub struct TestDb {
    server: PgConnectOptions,
    name: String,
}

impl TestDb {
    /// Creates a migrated database, with the accounts tests use, on the
    /// server named by `BEEP_TEST_DATABASE_URL`.
    pub async fn new() -> Result<Self> {
        let server = server()?;
        let mut admin = PgConnection::connect_with(&server).await?;
        let template = template(&server, &mut admin).await?;
        Self::create(server, admin, &format!("TEMPLATE {template}")).await
    }

    /// An empty database, for tests that migrate it part of the way.
    #[cfg(test)]
    async fn empty() -> Result<Self> {
        let server = server()?;
        let admin = PgConnection::connect_with(&server).await?;
        Self::create(server, admin, "").await
    }

    async fn create(server: PgConnectOptions, mut admin: PgConnection, from: &str) -> Result<Self> {
        let name = format!("test_{}", Uuid::new_v4().simple());
        admin
            .execute(format!("CREATE DATABASE {name} {from}").as_str())
            .await?;
        Ok(Self { server, name })
    }

    /// Opens a store on this database. Opening it again sees the same data.
    pub async fn store(&self) -> Result<Store> {
        self.store_with(Arc::new(WordEmbedder)).await
    }

    /// An empty database migrated as it was before chats had message rows,
    /// with the accounts tests use. [`Self::store`] then migrates it the
    /// rest of the way.
    #[cfg(test)]
    pub(crate) async fn legacy() -> Result<(Self, PgPool)> {
        let db = Self::empty().await?;
        let pool = db.pool().await?;
        super::migrations::migrator(super::migrations::BEFORE_BACKFILL - 1)
            .run(&pool)
            .await?;
        add_users(&pool).await?;
        Ok((db, pool))
    }

    /// An empty database migrated up to `last`, for testing the migration
    /// after it. [`Self::store`] then migrates the rest.
    #[cfg(test)]
    pub(crate) async fn at(last: i64) -> Result<(Self, PgPool)> {
        let db = Self::empty().await?;
        let pool = db.pool().await?;
        super::migrations::migrator(last).run(&pool).await?;
        Ok((db, pool))
    }

    /// A pool on this database, for tests that check rows no store method
    /// reads back.
    #[doc(hidden)]
    pub async fn raw_pool(&self) -> Result<PgPool> {
        self.pool().await
    }

    async fn pool(&self) -> Result<PgPool> {
        Ok(PgPoolOptions::new()
            .max_connections(4)
            .connect_with(self.server.clone().database(&self.name))
            .await?)
    }

    /// [`Self::store`] with a real embedder, for evals that need memory
    /// search by meaning.
    pub async fn store_with(&self, embedder: Arc<dyn EmbeddingProvider>) -> Result<Store> {
        let store = Store::with_pool(self.pool().await?, embedder).await?;
        add_users(&store.pool).await?;
        Ok(store)
    }
}

/// The server `BEEP_TEST_DATABASE_URL` names, connected to its database.
fn server() -> Result<PgConnectOptions> {
    let url = std::env::var(URL_VAR)
        .with_context(|| format!("{URL_VAR} is not set; run `just test-db` for one"))?;
    Ok(PgConnectOptions::from_str(&url)?)
}

/// The migrated database every [`TestDb::new`] copies, built once per set
/// of migrations and accounts. Test processes share it, so one builds it while the
/// others wait on a lock.
async fn template(server: &PgConnectOptions, admin: &mut PgConnection) -> Result<String> {
    let mut hash = blake3::Hasher::new();
    for (version, _, sql) in super::MIGRATIONS {
        hash.update(&version.to_le_bytes());
        hash.update(sql.as_bytes());
    }
    for user in TEST_USERS {
        hash.update(user.as_bytes());
    }
    let name = format!("beep_template_{}", &hash.finalize().to_hex()[..16]);
    let ready = "SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname = $1 AND datistemplate)";
    if sqlx::query_scalar(ready)
        .bind(&name)
        .fetch_one(&mut *admin)
        .await?
    {
        return Ok(name);
    }
    sqlx::query("SELECT pg_advisory_lock(hashtext($1))")
        .bind(&name)
        .execute(&mut *admin)
        .await?;
    let built = build(server, admin, &name, ready).await;
    sqlx::query("SELECT pg_advisory_unlock(hashtext($1))")
        .bind(&name)
        .execute(&mut *admin)
        .await?;
    built.map(|()| name)
}

async fn build(
    server: &PgConnectOptions,
    admin: &mut PgConnection,
    name: &str,
    ready: &str,
) -> Result<()> {
    if sqlx::query_scalar(ready)
        .bind(name)
        .fetch_one(&mut *admin)
        .await?
    {
        return Ok(());
    }
    // What a process killed mid-build left.
    admin
        .execute(format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)").as_str())
        .await?;
    admin
        .execute(format!("CREATE DATABASE {name}").as_str())
        .await?;
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(server.clone().database(name))
        .await?;
    // The memory store creates it on opening, but once here is cheaper.
    pool.execute("CREATE EXTENSION vector").await?;
    super::migrate(&pool).await?;
    add_users(&pool).await?;
    pool.close().await;
    // Nobody connects to it after this, since a copy cannot start while
    // anyone is.
    admin
        .execute(
            format!("ALTER DATABASE {name} WITH IS_TEMPLATE true ALLOW_CONNECTIONS false").as_str(),
        )
        .await?;
    Ok(())
}

async fn add_users(pool: &PgPool) -> Result<()> {
    sqlx::query(
        "INSERT INTO users (id, name, external_id) SELECT id, id, id FROM unnest($1::text[]) id \
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(TEST_USERS)
    .execute(pool)
    .await?;
    Ok(())
}

impl Drop for TestDb {
    fn drop(&mut self) {
        let (server, name) = (self.server.clone(), self.name.clone());
        let (closed_tx, closed) = mpsc::channel();
        // Drop runs inside the test's runtime, which cannot block on a
        // query, so the database goes from a thread with a runtime of its
        // own. The test waits only until it is closed to connections:
        // DROP DATABASE waits for every connection still starting up, and
        // one the test's own tasks began cannot finish until the test
        // returns and its runtime goes.
        thread::spawn(move || {
            let dropped = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(anyhow::Error::from)
                .and_then(|runtime| {
                    runtime.block_on(async {
                        let mut admin = PgConnection::connect_with(&server).await?;
                        admin
                            .execute(
                                format!("ALTER DATABASE {name} WITH ALLOW_CONNECTIONS false")
                                    .as_str(),
                            )
                            .await?;
                        closed_tx.send(())?;
                        reap(&mut admin, &name).await
                    })
                });
            if let Err(error) = dropped {
                log::warn!("unable to drop test database {name}: {error:#}");
            }
        });
        if closed.recv().is_err() {
            log::warn!("unable to close test database {}", self.name);
        }
    }
}

/// Drops every test database closed to connections, `name` first. The
/// others are ones a test process exited before dropping.
async fn reap(admin: &mut PgConnection, name: &str) -> Result<()> {
    let closed: Vec<String> = sqlx::query_scalar(
        "SELECT datname FROM pg_database WHERE datname LIKE 'test\\_%' AND NOT datallowconn
         ORDER BY datname = $1 DESC",
    )
    .bind(name)
    .fetch_all(&mut *admin)
    .await?;
    for name in closed {
        admin
            .execute(format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)").as_str())
            .await?;
    }
    Ok(())
}
