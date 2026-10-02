use anyhow::{Result, bail};
use flashmind::core::Conversation;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio_rusqlite::Connection;
use uuid::Uuid;

#[derive(Clone)]
pub struct Store(Connection);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub seq: i64,
    pub run_id: String,
    #[serde(flatten)]
    pub data: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub id: String,
    pub session_id: String,
    pub status: String,
}

pub enum CreateRun {
    Created(Run),
    Existing(Run),
    Conflict,
}

impl Store {
    pub async fn open(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let db = Connection::open(path).await?;
        db.call::<_, _, rusqlite::Error>(|db| {
            db.execute_batch(
                "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;
                CREATE TABLE IF NOT EXISTS sessions (
                    user_id TEXT NOT NULL, id TEXT NOT NULL, conversation TEXT NOT NULL,
                    PRIMARY KEY(user_id, id));
                CREATE TABLE IF NOT EXISTS runs (
                    id TEXT PRIMARY KEY, user_id TEXT NOT NULL, session_id TEXT NOT NULL,
                    request_id TEXT NOT NULL, message TEXT NOT NULL, status TEXT NOT NULL,
                    UNIQUE(user_id, request_id));
                CREATE UNIQUE INDEX IF NOT EXISTS one_active_run
                    ON runs(user_id, session_id) WHERE status = 'running';
                CREATE TABLE IF NOT EXISTS events (
                    run_id TEXT NOT NULL REFERENCES runs(id), seq INTEGER NOT NULL,
                    data TEXT NOT NULL, PRIMARY KEY(run_id, seq));
                CREATE TABLE IF NOT EXISTS memories (
                    user_id TEXT NOT NULL, key TEXT NOT NULL, content TEXT NOT NULL,
                    PRIMARY KEY(user_id, key));
                CREATE TABLE IF NOT EXISTS crons (
                    user_id TEXT NOT NULL, id TEXT NOT NULL, label TEXT NOT NULL,
                    prompt TEXT NOT NULL, schedule TEXT NOT NULL,
                    enabled INTEGER NOT NULL DEFAULT 1,
                    run_once INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                    PRIMARY KEY(user_id, id));
                CREATE TABLE IF NOT EXISTS cron_logs (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    user_id TEXT NOT NULL, cron_id TEXT NOT NULL,
                    run_id TEXT, started_at TEXT NOT NULL, finished_at TEXT,
                    status TEXT NOT NULL DEFAULT 'running',
                    error TEXT NOT NULL DEFAULT '');
                CREATE TABLE IF NOT EXISTS devices (
                    user_id TEXT NOT NULL, device_id TEXT NOT NULL, fcm_token TEXT NOT NULL,
                    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                    PRIMARY KEY(user_id, device_id));
                CREATE TABLE IF NOT EXISTS locations (
                    user_id TEXT PRIMARY KEY, latitude REAL NOT NULL, longitude REAL NOT NULL,
                    accuracy REAL NOT NULL, timezone TEXT NOT NULL DEFAULT 'UTC',
                    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')));
                CREATE TABLE IF NOT EXISTS mcp_connections (
                    user_id TEXT NOT NULL, id TEXT NOT NULL,
                    provider TEXT NOT NULL, label TEXT NOT NULL,
                    config TEXT NOT NULL DEFAULT '{}',
                    credentials TEXT NOT NULL DEFAULT '{}',
                    status TEXT NOT NULL DEFAULT 'disconnected',
                    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                    PRIMARY KEY(user_id, id));
                CREATE INDEX IF NOT EXISTS idx_cron_logs_user_cron
                    ON cron_logs(user_id, cron_id);
                CREATE INDEX IF NOT EXISTS idx_runs_user_session
                    ON runs(user_id, session_id);",
            )?;
            // Migrations for existing databases
            let has_col = |table: &str, col: &str| -> rusqlite::Result<bool> {
                let mut stmt = db.prepare(&format!("PRAGMA table_info({})", table))?;
                let found = stmt.query_map([], |r| r.get::<_, String>(1))?.any(|r| r.as_deref() == Ok(col));
                Ok(found)
            };
            if !has_col("crons", "run_once")? {
                db.execute_batch("ALTER TABLE crons ADD COLUMN run_once INTEGER NOT NULL DEFAULT 0;")?;
            }
            if !has_col("locations", "timezone")? {
                db.execute_batch("ALTER TABLE locations ADD COLUMN timezone TEXT NOT NULL DEFAULT 'UTC';")?;
            }
            Ok(())
        })
        .await?;
        Ok(Self(db))
    }

    // Called once before accepting requests. A server restart never silently repeats tools.
    pub async fn recover(&self) -> Result<()> {
        self.0
            .call::<_, _, rusqlite::Error>(|db| {
                let tx = db.transaction()?;
                tx.execute(
                    "INSERT INTO events (run_id, seq, data)
                SELECT id, COALESCE((SELECT MAX(seq) FROM events WHERE run_id=runs.id), 0)+1,
                '{\"type\":\"run_finished\",\"status\":\"interrupted\"}'
                FROM runs WHERE status='running'",
                    [],
                )?;
                tx.execute(
                    "UPDATE runs SET status='interrupted' WHERE status='running'",
                    [],
                )?;
                tx.commit()?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    pub async fn create_run(
        &self,
        user: &str,
        session: &str,
        request: &str,
        message: &str,
    ) -> Result<CreateRun> {
        let (user, session, request, message) = (
            user.to_owned(),
            session.to_owned(),
            request.to_owned(),
            message.to_owned(),
        );
        Ok(self.0.call::<_, _, rusqlite::Error>(move |db| {
            let tx = db.transaction()?;
            let existing = tx.query_row(
                "SELECT id, session_id, status, message FROM runs WHERE user_id=? AND request_id=?",
                params![user, request], |r| Ok((Run { id:r.get(0)?, session_id:r.get(1)?, status:r.get(2)? }, r.get::<_,String>(3)?))
            ).optional()?;
            if let Some((run, original)) = existing {
                return Ok(if run.session_id == session && original == message { CreateRun::Existing(run) } else { CreateRun::Conflict });
            }
            let busy: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM runs WHERE user_id=? AND session_id=? AND status='running')", params![user, session], |r| r.get(0))?;
            if busy { return Ok(CreateRun::Conflict); }
            let active: i64 = tx.query_row("SELECT COUNT(*) FROM runs WHERE status='running'", [], |r| r.get(0))?;
            if active >= 16 { return Ok(CreateRun::Conflict); }
            let run = Run { id: Uuid::new_v4().to_string(), session_id: session, status: "running".into() };
            tx.execute("INSERT INTO runs VALUES (?, ?, ?, ?, ?, 'running')", params![run.id, user, run.session_id, request, message])?;
            tx.execute("INSERT INTO events VALUES (?, 1, ?)", params![run.id, json!({"type":"run_started", "session_id":run.session_id}).to_string()])?;
            tx.commit()?;
            Ok(CreateRun::Created(run))
        }).await?)
    }

    pub async fn run(&self, user: &str, id: &str) -> Result<Option<Run>> {
        let (user, id) = (user.to_owned(), id.to_owned());
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                db.query_row(
                    "SELECT id, session_id, status FROM runs WHERE user_id=? AND id=?",
                    params![user, id],
                    |r| {
                        Ok(Run {
                            id: r.get(0)?,
                            session_id: r.get(1)?,
                            status: r.get(2)?,
                        })
                    },
                )
                .optional()
            })
            .await?)
    }

    pub async fn sessions(&self, user: &str) -> Result<Vec<Value>> {
        let user = user.to_owned();
        Ok(self.0.call::<_, _, rusqlite::Error>(move |db| {
            let mut statement = db.prepare(
                "SELECT s.id, COALESCE((SELECT substr(message,1,80) FROM runs r WHERE r.user_id=s.user_id AND r.session_id=s.id ORDER BY r.rowid LIMIT 1), 'Conversation') FROM sessions s WHERE s.user_id=? ORDER BY s.rowid DESC LIMIT 200"
            )?;
            let rows = statement.query_map([user], |row| Ok(json!({"id":row.get::<_, String>(0)?, "title":row.get::<_, String>(1)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        }).await?)
    }

    pub async fn conversation(&self, user: &str, session: &str) -> Result<Option<Conversation>> {
        let (user, session) = (user.to_owned(), session.to_owned());
        let raw: Option<String> = self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                db.query_row(
                    "SELECT conversation FROM sessions WHERE user_id=? AND id=?",
                    params![user, session],
                    |r| r.get(0),
                )
                .optional()
            })
            .await?;
        raw.map(|s| serde_json::from_str(&s).map_err(Into::into))
            .transpose()
    }

    pub async fn append(&self, run: &str, data: Value) -> Result<()> {
        let (run, data) = (run.to_owned(), data.to_string());
        self.0.call::<_, _, rusqlite::Error>(move |db| {
            db.execute("INSERT INTO events SELECT ?, COALESCE(MAX(seq),0)+1, ? FROM events WHERE run_id=?", params![run,data,run])?;
            Ok(())
        }).await?;
        Ok(())
    }

    pub async fn events(&self, run: &str, after: i64) -> Result<Vec<Event>> {
        let run = run.to_owned();
        let rows = self.0.call::<_, _, rusqlite::Error>(move |db| {
            let mut stmt = db.prepare("SELECT seq, run_id, data FROM events WHERE run_id=? AND seq>? ORDER BY seq LIMIT 256")?;
            let rows = stmt.query_map(params![run,after], |r| Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        }).await?;
        rows.into_iter()
            .map(|(seq, run_id, data)| {
                Ok(Event {
                    seq,
                    run_id,
                    data: serde_json::from_str(&data)?,
                })
            })
            .collect()
    }

    pub async fn finish(
        &self,
        user: &str,
        run: &Run,
        conversation: Option<&Conversation>,
        status: &str,
    ) -> Result<()> {
        let (user, run, status) = (user.to_owned(), run.clone(), status.to_owned());
        let conversation = conversation.map(serde_json::to_string).transpose()?;
        self.0.call::<_, _, rusqlite::Error>(move |db| {
            let tx = db.transaction()?;
            if let Some(conversation) = conversation {
                tx.execute("INSERT INTO sessions VALUES (?, ?, ?) ON CONFLICT(user_id,id) DO UPDATE SET conversation=excluded.conversation", params![user,run.session_id,conversation])?;
            }
            tx.execute("INSERT INTO events SELECT ?, COALESCE(MAX(seq),0)+1, ? FROM events WHERE run_id=?", params![run.id,json!({"type":"run_finished","status":status}).to_string(),run.id])?;
            tx.execute("UPDATE runs SET status=? WHERE id=? AND user_id=?", params![status,run.id,user])?;
            tx.commit()?;
            Ok(())
        }).await?;
        Ok(())
    }

    pub async fn remember(&self, user: &str, key: &str, content: &str) -> Result<()> {
        if key.is_empty() || key.len() > 100 || content.is_empty() || content.len() > 4000 {
            bail!("memory key/content length out of range");
        }
        let (user, key, content) = (user.to_owned(), key.to_owned(), content.to_owned());
        self.0.call::<_, _, rusqlite::Error>(move |db| {
            db.execute("INSERT INTO memories VALUES (?, ?, ?) ON CONFLICT(user_id,key) DO UPDATE SET content=excluded.content",params![user,key,content])?;
            Ok(())
        }).await?;
        Ok(())
    }

    pub async fn recall(&self, user: &str, query: &str) -> Result<Value> {
        let (user, query) = (user.to_owned(), query.to_owned());
        Ok(self.0.call::<_, _, rusqlite::Error>(move |db| {
            let mut stmt = db.prepare("SELECT key,content FROM memories WHERE user_id=? AND (instr(lower(key),lower(?))>0 OR instr(lower(content),lower(?))>0) ORDER BY key LIMIT 50")?;
            let rows = stmt.query_map(params![user,query,query], |r| Ok(json!({"key":r.get::<_,String>(0)?,"content":r.get::<_,String>(1)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(json!(rows))
        }).await?)
    }

    pub async fn forget(&self, user: &str, key: &str) -> Result<bool> {
        let (user, key) = (user.to_owned(), key.to_owned());
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                Ok(db.execute(
                    "DELETE FROM memories WHERE user_id=? AND key=?",
                    params![user, key],
                )? > 0)
            })
            .await?)
    }

    pub async fn list_memories(&self, user: &str) -> Result<Vec<Value>> {
        let user = user.to_owned();
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                let mut stmt = db.prepare(
                    "SELECT key, content FROM memories WHERE user_id=? ORDER BY key LIMIT 500",
                )?;
                let rows = stmt
                    .query_map([user], |r| {
                        Ok(json!({"key":r.get::<_,String>(0)?,"content":r.get::<_,String>(1)?}))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await?)
    }

    pub async fn list_crons(&self, user: &str) -> Result<Vec<Value>> {
        let user = user.to_owned();
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                let mut stmt = db.prepare(
                    "SELECT id, label, prompt, schedule, enabled, created_at, run_once FROM crons WHERE user_id=? ORDER BY created_at DESC LIMIT 100",
                )?;
                let rows = stmt
                    .query_map([user], |r| {
                        Ok(json!({
                            "id": r.get::<_,String>(0)?,
                            "label": r.get::<_,String>(1)?,
                            "prompt": r.get::<_,String>(2)?,
                            "schedule": r.get::<_,String>(3)?,
                            "enabled": r.get::<_,bool>(4)?,
                            "created_at": r.get::<_,String>(5)?,
                            "run_once": r.get::<_,bool>(6)?,
                        }))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await?)
    }

    pub async fn create_cron(
        &self,
        user: &str,
        label: &str,
        schedule: &str,
        prompt: &str,
    ) -> Result<String> {
        self.create_cron_ex(user, label, schedule, prompt, false).await
    }

    pub async fn create_cron_ex(
        &self,
        user: &str,
        label: &str,
        schedule: &str,
        prompt: &str,
        run_once: bool,
    ) -> Result<String> {
        if label.is_empty() || label.len() > 200 {
            bail!("cron label must be 1–200 characters");
        }
        if prompt.is_empty() || prompt.len() > 4000 {
            bail!("cron prompt must be 1–4000 characters");
        }
        if schedule.is_empty() || schedule.len() > 100 {
            bail!("cron schedule must be 1–100 characters");
        }
        let id = Uuid::new_v4().to_string();
        let (user, id_clone, label, prompt, schedule) = (
            user.to_owned(),
            id.clone(),
            label.to_owned(),
            prompt.to_owned(),
            schedule.to_owned(),
        );
        self.0
            .call::<_, _, rusqlite::Error>(move |db| {
                db.execute(
                    "INSERT INTO crons (user_id, id, label, prompt, schedule, run_once) VALUES (?, ?, ?, ?, ?, ?)",
                    params![user, id_clone, label, prompt, schedule, run_once],
                )?;
                Ok(())
            })
            .await?;
        Ok(id)
    }

    pub async fn update_cron(
        &self,
        user: &str,
        id: &str,
        enabled: Option<bool>,
        label: Option<&str>,
        schedule: Option<&str>,
    ) -> Result<bool> {
        let (user, id) = (user.to_owned(), id.to_owned());
        let set_enabled = enabled;
        let label = label.map(|s| s.to_owned());
        let schedule = schedule.map(|s| s.to_owned());
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                let tx = db.transaction()?;
                let mut changed = false;
                if let Some(enabled) = set_enabled {
                    changed |= tx.execute(
                        "UPDATE crons SET enabled=? WHERE user_id=? AND id=?",
                        params![enabled, user, id],
                    )? > 0;
                }
                if let Some(label) = &label {
                    changed |= tx.execute(
                        "UPDATE crons SET label=? WHERE user_id=? AND id=?",
                        params![label, user, id],
                    )? > 0;
                }
                if let Some(schedule) = &schedule {
                    changed |= tx.execute(
                        "UPDATE crons SET schedule=? WHERE user_id=? AND id=?",
                        params![schedule, user, id],
                    )? > 0;
                }
                tx.commit()?;
                Ok(changed)
            })
            .await?)
    }

    pub async fn delete_cron(&self, user: &str, id: &str) -> Result<bool> {
        let (user, id) = (user.to_owned(), id.to_owned());
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                Ok(db.execute(
                    "DELETE FROM crons WHERE user_id=? AND id=?",
                    params![user, id],
                )? > 0)
            })
            .await?)
    }

    pub async fn register_device(&self, user: &str, device_id: &str, fcm_token: &str) -> Result<()> {
        let (user, device_id, fcm_token) = (user.to_owned(), device_id.to_owned(), fcm_token.to_owned());
        self.0
            .call::<_, _, rusqlite::Error>(move |db| {
                db.execute(
                    "INSERT INTO devices (user_id, device_id, fcm_token) VALUES (?, ?, ?) ON CONFLICT(user_id, device_id) DO UPDATE SET fcm_token=excluded.fcm_token, created_at=strftime('%Y-%m-%dT%H:%M:%SZ','now')",
                    params![user, device_id, fcm_token],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    pub async fn unregister_device(&self, user: &str, device_id: &str) -> Result<bool> {
        let (user, device_id) = (user.to_owned(), device_id.to_owned());
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                Ok(db.execute(
                    "DELETE FROM devices WHERE user_id=? AND device_id=?",
                    params![user, device_id],
                )? > 0)
            })
            .await?)
    }

    pub async fn due_crons(&self) -> Result<Vec<(String, Value)>> {
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(|db| {
                let mut stmt = db.prepare(
                    "SELECT user_id, id, label, prompt, schedule, run_once FROM crons WHERE enabled=1",
                )?;
                let rows = stmt
                    .query_map([], |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            json!({
                                "id": r.get::<_, String>(1)?,
                                "label": r.get::<_, String>(2)?,
                                "prompt": r.get::<_, String>(3)?,
                                "schedule": r.get::<_, String>(4)?,
                                "run_once": r.get::<_, bool>(5)?,
                            }),
                        ))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await?)
    }

    pub async fn log_cron_start(&self, user: &str, cron_id: &str, run_id: &str) -> Result<()> {
        let (user, cron_id, run_id) = (user.to_owned(), cron_id.to_owned(), run_id.to_owned());
        self.0
            .call::<_, _, rusqlite::Error>(move |db| {
                db.execute(
                    "INSERT INTO cron_logs (user_id, cron_id, run_id, started_at, status) VALUES (?, ?, ?, datetime('now'), 'running')",
                    params![user, cron_id, run_id],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    pub async fn log_cron_finish(&self, run_id: &str, status: &str, error: &str) -> Result<()> {
        let (run_id, status, error) = (run_id.to_owned(), status.to_owned(), error.to_owned());
        self.0
            .call::<_, _, rusqlite::Error>(move |db| {
                db.execute(
                    "UPDATE cron_logs SET finished_at=datetime('now'), status=?, error=? WHERE run_id=?",
                    params![status, error, run_id],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    pub async fn cron_last_run(&self, user: &str, cron_id: &str) -> Result<Option<String>> {
        let (user, cron_id) = (user.to_owned(), cron_id.to_owned());
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                db.query_row(
                    "SELECT started_at FROM cron_logs WHERE user_id=? AND cron_id=? ORDER BY id DESC LIMIT 1",
                    params![user, cron_id],
                    |r| r.get(0),
                )
                .optional()
            })
            .await?)
    }

    pub async fn device_tokens(&self, user: &str) -> Result<Vec<String>> {
        let user = user.to_owned();
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                let mut stmt = db.prepare("SELECT fcm_token FROM devices WHERE user_id=?")?;
                let rows = stmt
                    .query_map([user], |r| r.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await?)
    }

    pub async fn list_mcp_connections(&self, user: &str) -> Result<Vec<Value>> {
        let user = user.to_owned();
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                let mut stmt = db.prepare(
                    "SELECT id, provider, label, status, created_at FROM mcp_connections WHERE user_id=? ORDER BY created_at DESC LIMIT 100",
                )?;
                let rows = stmt
                    .query_map([user], |r| {
                        Ok(json!({
                            "id": r.get::<_,String>(0)?,
                            "provider": r.get::<_,String>(1)?,
                            "label": r.get::<_,String>(2)?,
                            "status": r.get::<_,String>(3)?,
                            "created_at": r.get::<_,String>(4)?,
                        }))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await?)
    }

    #[allow(dead_code)]
    pub async fn get_mcp_connection(&self, user: &str, id: &str) -> Result<Option<Value>> {
        let (user, id) = (user.to_owned(), id.to_owned());
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                db.query_row(
                    "SELECT id, provider, label, config, credentials, status FROM mcp_connections WHERE user_id=? AND id=?",
                    params![user, id],
                    |r| {
                        Ok(json!({
                            "id": r.get::<_,String>(0)?,
                            "provider": r.get::<_,String>(1)?,
                            "label": r.get::<_,String>(2)?,
                            "config": r.get::<_,String>(3)?,
                            "credentials": r.get::<_,String>(4)?,
                            "status": r.get::<_,String>(5)?,
                        }))
                    },
                )
                .optional()
            })
            .await?)
    }

    pub async fn create_mcp_connection(
        &self,
        user: &str,
        provider: &str,
        label: &str,
        config: &str,
    ) -> Result<String> {
        if provider.is_empty() || provider.len() > 100 {
            bail!("provider must be 1–100 characters");
        }
        if label.is_empty() || label.len() > 200 {
            bail!("label must be 1–200 characters");
        }
        if serde_json::from_str::<Value>(config).is_err() {
            bail!("config must be valid JSON");
        }
        let id = Uuid::new_v4().to_string();
        let (user, id_clone, provider, label, config) = (
            user.to_owned(),
            id.clone(),
            provider.to_owned(),
            label.to_owned(),
            config.to_owned(),
        );
        self.0
            .call::<_, _, rusqlite::Error>(move |db| {
                db.execute(
                    "INSERT INTO mcp_connections (user_id, id, provider, label, config) VALUES (?, ?, ?, ?, ?)",
                    params![user, id_clone, provider, label, config],
                )?;
                Ok(())
            })
            .await?;
        Ok(id)
    }

    pub async fn update_mcp_credentials(
        &self,
        user: &str,
        id: &str,
        credentials: &str,
        status: &str,
    ) -> Result<bool> {
        let (user, id, credentials, status) = (
            user.to_owned(),
            id.to_owned(),
            credentials.to_owned(),
            status.to_owned(),
        );
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                Ok(db.execute(
                    "UPDATE mcp_connections SET credentials=?, status=? WHERE user_id=? AND id=?",
                    params![credentials, status, user, id],
                )? > 0)
            })
            .await?)
    }

    pub async fn delete_mcp_connection(&self, user: &str, id: &str) -> Result<bool> {
        let (user, id) = (user.to_owned(), id.to_owned());
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                Ok(db.execute(
                    "DELETE FROM mcp_connections WHERE user_id=? AND id=?",
                    params![user, id],
                )? > 0)
            })
            .await?)
    }

    pub async fn update_location(
        &self,
        user: &str,
        latitude: f64,
        longitude: f64,
        accuracy: f64,
    ) -> Result<()> {
        self.update_location_tz(user, latitude, longitude, accuracy, "UTC").await
    }

    pub async fn update_location_tz(
        &self,
        user: &str,
        latitude: f64,
        longitude: f64,
        accuracy: f64,
        timezone: &str,
    ) -> Result<()> {
        let user = user.to_owned();
        let timezone = timezone.to_owned();
        self.0
            .call::<_, _, rusqlite::Error>(move |db| {
                db.execute(
                    "INSERT INTO locations (user_id, latitude, longitude, accuracy, timezone, updated_at) VALUES (?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%SZ','now')) ON CONFLICT(user_id) DO UPDATE SET latitude=excluded.latitude, longitude=excluded.longitude, accuracy=excluded.accuracy, timezone=excluded.timezone, updated_at=excluded.updated_at",
                    params![user, latitude, longitude, accuracy, timezone],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    pub async fn get_location(&self, user: &str) -> Result<Option<Value>> {
        let user = user.to_owned();
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                db.query_row(
                    "SELECT latitude, longitude, accuracy, timezone, updated_at FROM locations WHERE user_id=?",
                    params![user],
                    |r| {
                        Ok(json!({
                            "latitude": r.get::<_, f64>(0)?,
                            "longitude": r.get::<_, f64>(1)?,
                            "accuracy": r.get::<_, f64>(2)?,
                            "timezone": r.get::<_, String>(3)?,
                            "updated_at": r.get::<_, String>(4)?,
                        }))
                    },
                )
                .optional()
            })
            .await?)
    }

    pub async fn get_timezone(&self, user: &str) -> Result<String> {
        let user = user.to_owned();
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                db.query_row(
                    "SELECT timezone FROM locations WHERE user_id=?",
                    params![user],
                    |r| r.get(0),
                )
                .optional()
                .map(|v| v.unwrap_or_else(|| "UTC".into()))
            })
            .await?)
    }
}
