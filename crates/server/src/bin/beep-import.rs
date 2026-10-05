//! Copies a server's SQLite databases into the Postgres database named by
//! its config. Run once, against an empty database.
//!
//! Rows go in rowid order, so the Postgres insertion order matches. Values
//! are passed as text and cast to each column's type. Memories are embedded
//! again with the configured embedder and keep their ids and times.

use anyhow::{Context, Result, bail, ensure};
use beep_server::{config::Config, memories, store::Store};
use flashmind::memory::EmbeddingProvider;
use rusqlite::{
    Connection, OpenFlags,
    types::{Type, ValueRef},
};
use secrecy::ExposeSecret;
use sqlx::{PgConnection, Row, postgres::PgPoolOptions};
use std::{fs, path::Path};

const USAGE: &str = "usage: beep-import CONFIG SERVER_DB MEMORIES_DB";

/// Server tables, parents before children.
const TABLES: &[&str] = &[
    "sessions",
    "runs",
    "session_titles",
    "tool_failures",
    "memory_scores",
    "memory_curation",
    "crons",
    "devices",
    "locations",
    "mcp_servers",
    "notifications",
    "habits",
    "habit_logs",
    "mail_sync",
    "mail_seen",
];

/// A Postgres column to fill, with the type its text is cast to.
struct Column {
    name: String,
    pg_type: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    ensure!(args.len() == 3, USAGE);
    let config: Config = toml::from_str(
        &fs::read_to_string(&args[0]).with_context(|| format!("reading {}", args[0]))?,
    )?;
    let server_db = open_sqlite(Path::new(&args[1]))?;
    let memories_db = open_sqlite(Path::new(&args[2]))?;

    let url = config.database_url()?;
    let embedder = memories::embedder(&config)?;
    // Creates the schema, including the memory tables.
    drop(Store::open(&url, embedder.clone()).await?);

    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(url.expose_secret())
        .await?;
    let mut tx = pool.begin().await?;
    // Old times without an offset are UTC.
    sqlx::query("SET LOCAL TIME ZONE 'UTC'")
        .execute(&mut *tx)
        .await?;
    for table in TABLES.iter().copied().chain(["memories"]) {
        let rows: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(&mut *tx)
            .await?;
        ensure!(
            rows == 0,
            "{table} already has rows; import into an empty database"
        );
    }
    for table in TABLES {
        let copied = copy_table(&server_db, &mut tx, table).await?;
        println!("{table}: {copied}");
    }
    let copied = copy_memories(&memories_db, &mut tx, embedder.as_ref()).await?;
    println!("memories: {copied}");
    tx.commit().await?;
    Ok(())
}

fn open_sqlite(path: &Path) -> Result<Connection> {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("opening {}", path.display()))
}

/// The columns of `table` that take a value, leaving out generated ones.
async fn columns(pg: &mut PgConnection, table: &str) -> Result<Vec<Column>> {
    let rows = sqlx::query(
        "SELECT column_name::text AS name, format_type(a.atttypid, a.atttypmod) AS pg_type
         FROM information_schema.columns c
         JOIN pg_attribute a ON a.attrelid = to_regclass(c.table_name)
             AND a.attname = c.column_name
         WHERE c.table_name = $1 AND c.table_schema = current_schema()
             AND c.is_identity = 'NO' AND c.is_generated = 'NEVER'
         ORDER BY c.ordinal_position",
    )
    .bind(table)
    .fetch_all(&mut *pg)
    .await?;
    rows.iter()
        .map(|row| {
            Ok(Column {
                name: row.try_get("name")?,
                pg_type: row.try_get("pg_type")?,
            })
        })
        .collect()
}

/// Reads every row of `table` as text, in rowid order.
fn read_rows(db: &Connection, table: &str, columns: &[Column]) -> Result<Vec<Vec<Option<String>>>> {
    let names: Vec<&str> = columns.iter().map(|c| c.name.as_str()).collect();
    let mut stmt = db.prepare(&format!(
        "SELECT {} FROM {table} ORDER BY rowid",
        names.join(", ")
    ))?;
    let rows = stmt.query_map([], |row| {
        (0..names.len())
            .map(|i| {
                Ok(match row.get_ref(i)? {
                    ValueRef::Null => None,
                    ValueRef::Integer(n) => Some(n.to_string()),
                    ValueRef::Real(x) => Some(x.to_string()),
                    ValueRef::Text(text) => Some(String::from_utf8_lossy(text).into_owned()),
                    ValueRef::Blob(_) => {
                        return Err(rusqlite::Error::InvalidColumnType(
                            i,
                            names[i].to_owned(),
                            Type::Blob,
                        ));
                    }
                })
            })
            .collect::<rusqlite::Result<Vec<_>>>()
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

async fn copy_table(db: &Connection, pg: &mut PgConnection, table: &str) -> Result<usize> {
    let columns = columns(pg, table).await?;
    if columns.is_empty() {
        bail!("{table} has no columns in Postgres");
    }
    let rows = read_rows(db, table, &columns)?;
    let names: Vec<&str> = columns.iter().map(|c| c.name.as_str()).collect();
    let values: Vec<String> = columns
        .iter()
        .enumerate()
        .map(|(i, c)| format!("${}::text::{}", i + 1, c.pg_type))
        .collect();
    let insert = format!(
        "INSERT INTO {table} ({}) VALUES ({})",
        names.join(", "),
        values.join(", ")
    );
    for row in &rows {
        let mut query = sqlx::query(&insert);
        for value in row {
            query = query.bind(value);
        }
        query
            .execute(&mut *pg)
            .await
            .with_context(|| format!("copying a row of {table}"))?;
    }
    Ok(rows.len())
}

struct OldMemory {
    id: String,
    content: String,
    created_at: i64,
    expires_at: Option<i64>,
}

async fn copy_memories(
    db: &Connection,
    pg: &mut PgConnection,
    embedder: &dyn EmbeddingProvider,
) -> Result<usize> {
    let memories = db
        .prepare("SELECT id, content, created_at, expires_at FROM memories ORDER BY rowid")?
        .query_map([], |row| {
            Ok(OldMemory {
                id: row.get(0)?,
                content: row.get(1)?,
                created_at: row.get(2)?,
                expires_at: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut meta =
        db.prepare("SELECT key, value FROM memory_meta WHERE memory_id = ?1 ORDER BY rowid")?;
    for memory in &memories {
        let embedding = embedder.embed(&memory.content).await?;
        let vector = format!(
            "[{}]",
            embedding
                .iter()
                .map(f32::to_string)
                .collect::<Vec<_>>()
                .join(",")
        );
        sqlx::query(
            "INSERT INTO memories (id, content, created_at, expires_at, embedding)
             VALUES ($1, $2, $3, $4, $5::text::vector)",
        )
        .bind(&memory.id)
        .bind(&memory.content)
        .bind(memory.created_at)
        .bind(memory.expires_at)
        .bind(vector)
        .execute(&mut *pg)
        .await?;
        let pairs = meta
            .query_map([&memory.id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (key, value) in pairs {
            sqlx::query("INSERT INTO memory_meta (memory_id, key, value) VALUES ($1, $2, $3)")
                .bind(&memory.id)
                .bind(key)
                .bind(value)
                .execute(&mut *pg)
                .await?;
        }
    }
    Ok(memories.len())
}
