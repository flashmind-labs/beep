//! A throwaway schema in the test database, so tests run in parallel
//! without seeing each other's rows.

use super::{Store, WordEmbedder};
use anyhow::{Context, Result};
use sqlx::{
    Connection, Executor, PgConnection,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use std::{str::FromStr, sync::Arc, thread};
use uuid::Uuid;

/// Names the database tests run against, such as
/// `postgres://beep:beep@127.0.0.1:55432/beep`.
const URL_VAR: &str = "BEEP_TEST_DATABASE_URL";

/// Accounts every test store has, since rows need an account to belong to.
const TEST_USERS: &[&str] = &["alice", "bob", "user"];

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
        // The vector type lives in `public` so every test schema sees it.
        // Tests starting together may race to create it.
        if let Err(error) = admin
            .execute("CREATE EXTENSION IF NOT EXISTS vector WITH SCHEMA public")
            .await
        {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_extension WHERE extname = 'vector')",
            )
            .fetch_one(&mut admin)
            .await?;
            if !exists {
                return Err(error.into());
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
        let options = PgConnectOptions::from_str(&self.url)?
            .options([("search_path", format!("{},public", self.schema))]);
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await?;
        let store = Store::with_pool(pool, Arc::new(WordEmbedder)).await?;
        sqlx::query(
            "INSERT INTO users (id, name, external_id) SELECT id, id, id FROM unnest($1::text[]) id \
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(TEST_USERS)
        .execute(&store.pool)
        .await?;
        Ok(store)
    }
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
