//! A throwaway schema in the test database, so tests run in parallel
//! without seeing each other's rows.

use super::{
    CreateRun, FinishedTurn, RunStatus, Store, WordEmbedder,
    messages::{Step, TurnRecord},
};
use crate::{steps, user::UserKey};
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
use std::{collections::HashMap, str::FromStr, sync::Arc, thread};
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

/// A schema of its own in the test database, dropped with this value.
#[doc(hidden)]
pub struct TestDb {
    url: String,
    schema: String,
}

impl TestDb {
    /// Creates an empty schema in the database named by
    /// `BEEP_TEST_DATABASE_URL`.
    pub async fn new() -> Result<Self> {
        let url = std::env::var(URL_VAR)
            .with_context(|| format!("{URL_VAR} is not set; run `just test-db` for one"))?;
        let schema = format!("test_{}", Uuid::new_v4().simple());
        let mut admin = PgConnection::connect(&url).await?;
        // Extensions live in `public` so every test schema sees them. Tests
        // starting together may race to create them.
        for extension in ["vector", "pg_trgm"] {
            if let Err(error) = admin
                .execute(
                    format!("CREATE EXTENSION IF NOT EXISTS {extension} WITH SCHEMA public")
                        .as_str(),
                )
                .await
            {
                let exists: bool = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM pg_extension WHERE extname = $1)",
                )
                .bind(extension)
                .fetch_one(&mut admin)
                .await?;
                if !exists {
                    return Err(error.into());
                }
            }
        }
        admin
            .execute(format!("CREATE SCHEMA {schema}").as_str())
            .await?;
        Ok(Self { url, schema })
    }

    /// Opens a store on this schema, with the accounts tests use. Opening it
    /// again sees the same data.
    pub async fn store(&self) -> Result<Store> {
        self.store_with(Arc::new(WordEmbedder)).await
    }

    /// A pool on this schema as it was before chats had message rows, with
    /// the accounts tests use. [`Self::store`] then migrates it the rest of
    /// the way.
    #[cfg(test)]
    pub(crate) async fn legacy_pool(&self) -> Result<PgPool> {
        let pool = self.pool().await?;
        super::migrator(super::BEFORE_BACKFILL - 1)
            .run(&pool)
            .await?;
        add_users(&pool).await?;
        Ok(pool)
    }

    /// A pool on this schema, for tests that check rows no store method
    /// reads back.
    #[doc(hidden)]
    pub async fn raw_pool(&self) -> Result<PgPool> {
        self.pool().await
    }

    async fn pool(&self) -> Result<PgPool> {
        let options = PgConnectOptions::from_str(&self.url)?
            .options([("search_path", format!("{},public", self.schema))]);
        Ok(PgPoolOptions::new()
            .max_connections(4)
            .connect_with(options)
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
        let (url, schema) = (self.url.clone(), self.schema.clone());
        // Drop runs inside the test's runtime, which cannot block on a
        // query, so the schema goes from a thread with a runtime of its own.
        let dropped = thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            runtime.block_on(async {
                let mut admin = PgConnection::connect(&url).await?;
                admin
                    .execute(format!("DROP SCHEMA IF EXISTS {schema} CASCADE").as_str())
                    .await?;
                anyhow::Ok(())
            })
        })
        .join();
        if !matches!(dropped, Ok(Ok(()))) {
            log::warn!("unable to drop test schema {}", self.schema);
        }
    }
}
