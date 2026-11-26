//! The search index on disk: each session's prompts and agent replies, kept
//! once, and the change log copies follow, which is the same table.

use anyhow::{Result, bail};
use beep_proto::{
    SessionId,
    node::agent::{IndexRow, SearchHit, SearchSession, folded_at},
};
use rusqlite::{Connection, OptionalExtension, functions::FunctionFlags, params};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, path::Path};

const SCHEMA: u32 = 2;
/// The most sessions one search answers with.
const MOST_HITS: u32 = 64;
/// About the most text one batch of rows carries, well under a frame.
const BATCH_BYTES: usize = 1 << 20;
/// The characters a hit's text keeps before and after its word.
const AROUND: usize = 200;

pub(super) struct Store(Connection);

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let mut db = Connection::open(path)?;
        db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;")?;
        // Whether `text` holds the lowercase `word`, with Unicode case
        // folding, which SQLite's LIKE does only for ASCII.
        db.create_scalar_function(
            "holds",
            2,
            FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
            |call| {
                let text: Option<String> = call.get(0)?;
                let word: String = call.get(1)?;
                Ok(text.is_some_and(|text| text.to_lowercase().contains(&word)))
            },
        )?;
        let version: u32 = db.pragma_query_value(None, "user_version", |row| row.get(0))?;

        if version > SCHEMA {
            bail!("the search index uses a newer schema");
        }

        if version < SCHEMA {
            if version == 0 {
                db.pragma_update(None, "auto_vacuum", "INCREMENTAL")?;
            }
            let tx = db.transaction()?;
            let epoch = format!("{:016x}", fastrand::u64(..));

            if version == 0 {
                tx.execute_batch(include_str!("schema.sql"))?;
                tx.execute("INSERT INTO meta(epoch) VALUES (?1)", [&epoch])?;
            } else {
                // Keep indexed text and replay the copy stream so upgraded
                // desktops also get metadata for text they already copied.
                tx.execute_batch("ALTER TABLE sessions ADD COLUMN metadata TEXT;")?;
                tx.execute("UPDATE meta SET epoch=?1", [&epoch])?;
            }

            tx.pragma_update(None, "user_version", SCHEMA)?;
            tx.commit()?;
            if version == 0 {
                db.execute_batch("VACUUM;")?;
            }
        }

        Ok(Self(db))
    }

    /// Names this index: copies of another start over.
    pub fn epoch(&self) -> Result<String> {
        Ok(self
            .0
            .query_row("SELECT epoch FROM meta", [], |row| row.get(0))?)
    }

    /// Publishes session details even when the transcript needs no reads.
    pub fn set_metadata(
        &mut self,
        id: &SessionId,
        activity: u64,
        metadata: &SearchSession,
    ) -> Result<bool> {
        let metadata = serde_json::to_string(metadata)?;
        let tx = self.0.transaction()?;
        let key: Option<i64> = tx
            .query_row(
                "INSERT INTO sessions(id,activity,metadata) VALUES (?1,?2,?3)
             ON CONFLICT(id) DO UPDATE SET activity=?2,metadata=?3
             WHERE activity<>?2 OR metadata IS NOT ?3 RETURNING key",
                params![id.as_ref(), i64::try_from(activity)?, metadata],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(key) = key {
            tx.execute("DELETE FROM log WHERE session=?1 AND text=''", [key])?;
            tx.execute("INSERT INTO log(session,text) VALUES (?1,'')", [key])?;
        }

        tx.commit()?;
        Ok(key.is_some())
    }

    /// Whether session `id` was fully indexed at `stamp`.
    pub fn current(&self, id: &SessionId, stamp: &str) -> Result<bool> {
        let saved: Option<bool> = self
            .0
            .query_row(
                "SELECT stamp=?2 AND walk_top IS NULL FROM sessions WHERE id=?1",
                [id.as_ref(), stamp],
                |row| row.get(0),
            )
            .optional()?;
        Ok(saved == Some(true))
    }

    /// How far session `id`'s transcript is indexed.
    pub fn progress(&self, id: &SessionId) -> Result<Progress> {
        let saved = self
            .0
            .query_row(
                "SELECT done,walk_top,walk_low FROM sessions WHERE id=?1",
                [id.as_ref()],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, Option<i64>>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                    ))
                },
            )
            .optional()?;
        let Some((done, top, low)) = saved else {
            return Ok(Progress::NEW);
        };
        let walk = match top.zip(low) {
            Some((top, low)) => Some(Walk {
                top: u64::try_from(top)?,
                low: u64::try_from(low)?,
            }),
            None => None,
        };

        Ok(Progress {
            done: u64::try_from(done)?,
            walk,
        })
    }

    /// Saves one transcript page's texts with how far the session is
    /// indexed after it, so an interrupted read resumes from there. The
    /// stamp is kept only when the read reached the newest entry and what
    /// it had already indexed: `current`.
    pub fn save(&mut self, session: &Indexed, texts: &[String], progress: Progress) -> Result<()> {
        let tx = self.0.unchecked_transaction()?;
        let (top, low) = progress.walk.map(|walk| (walk.top, walk.low)).unzip();
        let column = |at: Option<u64>| at.map(i64::try_from).transpose();
        tx.execute(
            "INSERT INTO sessions(id,stamp,activity,done,walk_top,walk_low) VALUES (?1,COALESCE(?2,''),?3,?4,?5,?6)
             ON CONFLICT(id) DO UPDATE SET stamp=COALESCE(?2,sessions.stamp),activity=?3,
                 done=?4,walk_top=?5,walk_low=?6",
            params![
                session.id.as_ref(),
                session.stamp,
                i64::try_from(session.activity)?,
                i64::try_from(progress.done)?,
                column(top)?,
                column(low)?,
            ],
        )?;
        {
            let mut insert = tx.prepare(
                "INSERT OR IGNORE INTO log(session,text,hash)
                 SELECT key,?2,?3 FROM sessions WHERE id=?1",
            )?;

            for text in texts {
                insert.execute(params![session.id.as_ref(), text, text_hash(text)])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Resets session `id`, as when it was deleted.
    pub fn forget(&mut self, id: &SessionId) -> Result<()> {
        let key: Option<i64> = self
            .0
            .query_row(
                "SELECT key FROM sessions WHERE id=?1",
                [id.as_ref()],
                |row| row.get(0),
            )
            .optional()?;
        let Some(key) = key else {
            return Ok(());
        };

        let tx = self.0.transaction()?;
        tx.execute("DELETE FROM log WHERE session=?1", [key])?;
        tx.execute("INSERT INTO log(session) VALUES (?1)", [key])?;
        tx.execute(
            "UPDATE sessions SET stamp='',done=0,walk_top=NULL,walk_low=NULL,metadata=NULL WHERE key=?1",
            [key],
        )?;

        tx.commit()?;
        self.0.execute_batch("PRAGMA incremental_vacuum;")?;
        Ok(())
    }

    /// Looks up a single session's metadata by its agent id or Beep managed id.
    pub fn session(&self, id: &SessionId) -> Result<Option<(SessionId, u64, SearchSession)>> {
        let row: Option<(String, i64, String)> = self
            .0
            .query_row(
                "SELECT id, activity, metadata FROM sessions WHERE id=?1 AND metadata IS NOT NULL",
                [id.as_ref()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;

        if let Some((found, activity, raw)) = row
            && let Ok(meta) = serde_json::from_str::<SearchSession>(&raw)
        {
            return Ok(Some((
                Into::<SessionId>::into(found),
                u64::try_from(activity).unwrap_or(0),
                meta,
            )));
        }

        // A Beep-managed id can differ from the harness id stored in `sessions.id`.
        let mut stmt = self.0.prepare(
            "SELECT id, activity, metadata FROM sessions
             WHERE metadata IS NOT NULL AND instr(metadata, ?1) > 0
             ORDER BY activity DESC",
        )?;
        let mut rows = stmt.query([id.as_ref()])?;

        while let Some(row) = rows.next()? {
            let found: String = row.get(0)?;
            let activity: i64 = row.get(1)?;
            let raw: String = row.get(2)?;

            if let Ok(meta) = serde_json::from_str::<SearchSession>(&raw)
                && meta.managed.as_ref() == Some(id)
            {
                return Ok(Some((
                    Into::<SessionId>::into(found),
                    u64::try_from(activity).unwrap_or(0),
                    meta,
                )));
            }
        }

        Ok(None)
    }

    /// The sessions holding every word of `query`, newest activity first;
    /// only those of `only` when it names any. A word of three characters
    /// or more finds its candidates by its trigrams.
    pub fn find(&self, query: &str, only: &[SessionId]) -> Result<Vec<SearchHit>> {
        let query = query.trim().to_lowercase();

        if query.chars().count() < 2 {
            return Ok(Vec::new());
        }

        let words: Vec<&str> = query
            .split_whitespace()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let mut parameters: Vec<String> = vec![serde_json::to_string(only)?];
        let mut conditions = Vec::new();
        let mut short = Vec::new();

        for word in &words {
            parameters.push((*word).to_owned());
            let holds = format!("holds(text,?{})", parameters.len());
            let title = format!(
                "holds(json_extract(metadata,'$.title'),?{})",
                parameters.len()
            );

            match trigrams(word) {
                Some(trigrams) => {
                    parameters.push(trigrams);
                    conditions.push(format!(
                        "(key IN (SELECT session FROM log WHERE seq IN
                         (SELECT rowid FROM log_search WHERE log_search MATCH ?{}) AND {holds}) OR {title})",
                        parameters.len()
                    ));
                }
                // Checked last, and only in the sessions the rest leave.
                None => short.push(format!(
                    "(EXISTS (SELECT 1 FROM log WHERE session=key AND {holds}) OR {title})"
                )),
            }
        }

        conditions.append(&mut short);

        let sql = format!(
            "SELECT key,id,activity,metadata FROM sessions
             WHERE (?1='[]' OR id IN (SELECT value FROM json_each(?1))) AND {}
             ORDER BY activity DESC,id LIMIT {MOST_HITS}",
            conditions.join(" AND ")
        );
        let mut read = self.0.prepare(&sql)?;
        let rows = read.query_map(rusqlite::params_from_iter(&parameters), |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })?;
        let mut first = self.0.prepare(
            "SELECT text FROM log WHERE session=?1 AND holds(text,?2) ORDER BY seq LIMIT 1",
        )?;

        rows.map(|row| {
            let (key, session, activity, metadata) = row?;
            let texts = words
                .iter()
                .map(|word| {
                    let text: Option<String> = first
                        .query_row(params![key, word], |row| row.get(0))
                        .optional()?;
                    Ok(text.map(|text| around(&text, word)))
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(SearchHit {
                session: SessionId::from(session),
                activity: u64::try_from(activity)?,
                texts: texts.into_iter().flatten().collect(),
                metadata: metadata.as_deref().map(serde_json::from_str).transpose()?,
            })
        })
        .collect()
    }

    /// The newest row's `seq`, 0 for none.
    pub fn newest(&self) -> Result<u64> {
        let seq: i64 = self
            .0
            .query_row("SELECT COALESCE(MAX(seq),0) FROM log", [], |row| row.get(0))?;
        Ok(u64::try_from(seq)?)
    }

    /// Up to `most` rows after `after`, in order, ending early once they
    /// carry about `BATCH_BYTES` of text.
    pub fn rows_after(&self, after: u64, most: u32) -> Result<Vec<IndexRow>> {
        let mut read = self.0.prepare(
            "SELECT seq,id,text,activity,CASE WHEN text='' THEN metadata END
             FROM log JOIN sessions ON key=session
             WHERE seq>?1 ORDER BY seq LIMIT ?2",
        )?;
        let rows = read.query_map(params![i64::try_from(after)?, most], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })?;

        let mut batch = Vec::new();
        let mut bytes = 0;

        for row in rows {
            let (seq, session, text, activity, metadata) = row?;
            bytes +=
                text.as_ref().map_or(0, String::len) + metadata.as_ref().map_or(0, String::len);
            batch.push(IndexRow {
                seq: u64::try_from(seq)?,
                session: SessionId::from(session),
                text,
                activity: u64::try_from(activity)?,
                metadata: metadata.as_deref().map(serde_json::from_str).transpose()?,
            });

            if bytes >= BATCH_BYTES {
                break;
            }
        }

        Ok(batch)
    }
}

/// A copy of another machine's index: the same tables, filled from the
/// rows its `FollowIndex` stream sends, so it is searched the same way.
pub struct IndexCopy(Store);

impl IndexCopy {
    pub fn open(path: &Path) -> Result<Self> {
        Ok(Self(Store::open(path)?))
    }

    /// The index this copies, and the newest row it holds.
    pub fn position(&self) -> Result<(String, u64)> {
        Ok((self.0.epoch()?, self.0.newest()?))
    }

    /// Applies rows of index `epoch` in order. Rows of another index than
    /// the one copied so far replace the copy.
    pub fn apply(&mut self, epoch: &str, rows: &[IndexRow]) -> Result<()> {
        let tx = self.0.0.unchecked_transaction()?;

        if epoch != self.0.epoch()? {
            tx.execute_batch("DELETE FROM log; DELETE FROM sessions;")?;
            tx.execute("UPDATE meta SET epoch=?1", [epoch])?;
        }

        let after = self.0.newest()?;
        for row in rows.iter().filter(|row| row.seq > after) {
            tx.execute(
                "INSERT INTO sessions(id,activity) VALUES (?1,?2)
                 ON CONFLICT(id) DO UPDATE SET activity=MAX(activity,?2)",
                params![row.session.as_ref(), i64::try_from(row.activity)?],
            )?;
            let key: i64 = tx.query_row(
                "SELECT key FROM sessions WHERE id=?1",
                [row.session.as_ref()],
                |found| found.get(0),
            )?;
            let seq = i64::try_from(row.seq)?;

            match (&row.text, &row.metadata) {
                (Some(text), Some(metadata)) if text.is_empty() => {
                    tx.execute(
                        "UPDATE sessions SET metadata=?1,activity=?2 WHERE key=?3",
                        params![
                            serde_json::to_string(metadata)?,
                            i64::try_from(row.activity)?,
                            key
                        ],
                    )?;
                    tx.execute("DELETE FROM log WHERE session=?1 AND text=''", [key])?;
                    tx.execute(
                        "INSERT INTO log(seq,session,text) VALUES (?1,?2,'')",
                        params![seq, key],
                    )?;
                }
                (Some(text), _) => {
                    tx.execute(
                        "INSERT OR IGNORE INTO log(seq,session,text,hash) VALUES (?1,?2,?3,?4)",
                        params![seq, key, text, text_hash(text)],
                    )?;
                }
                (None, _) => {
                    tx.execute("DELETE FROM log WHERE session=?1", [key])?;
                    tx.execute(
                        "INSERT INTO log(seq,session) VALUES (?1,?2)",
                        params![seq, key],
                    )?;
                    tx.execute("UPDATE sessions SET metadata=NULL WHERE key=?1", [key])?;
                }
            }
        }

        tx.commit()?;
        self.0.0.execute_batch("PRAGMA incremental_vacuum;")?;
        Ok(())
    }

    pub fn find(&self, query: &str, only: &[SessionId]) -> Result<Vec<SearchHit>> {
        self.0.find(query, only)
    }

    /// Looks up a single session's metadata by its agent id or Beep managed id.
    pub fn session(&self, id: &SessionId) -> Result<Option<(SessionId, u64, SearchSession)>> {
        self.0.session(id)
    }
}

/// An FTS5 query for every trigram of `word`, each quoted so it is taken
/// as written; `None` for a word too short to have one.
fn trigrams(word: &str) -> Option<String> {
    let chars: Vec<char> = word.chars().collect();
    let quoted: Vec<String> = chars
        .windows(3)
        .map(|three| {
            format!(
                "\"{}\"",
                three.iter().collect::<String>().replace('"', "\"\"")
            )
        })
        .collect();
    (!quoted.is_empty()).then(|| quoted.join(" AND "))
}

/// The part of `text` around the first place it holds the lowercase `word`,
/// so a hit stays small however long its text is.
fn around(text: &str, word: &str) -> String {
    let Some(at) = folded_at(text, word) else {
        return text.chars().take(2 * AROUND).collect();
    };
    let start = at.saturating_sub(AROUND);
    let keep = AROUND + word.chars().count() + AROUND;
    text.chars().skip(start).take(keep).collect()
}

/// Keeps a text unique in its session without a second copy of it.
fn text_hash(text: &str) -> i64 {
    let digest = Sha256::digest(text.as_bytes());
    let mut first = [0; 8];
    first.copy_from_slice(&digest[..8]);
    i64::from_le_bytes(first)
}

/// A session as one page of its transcript is saved.
pub(super) struct Indexed {
    /// The agent's own id for it.
    pub id: SessionId,
    /// Unix milliseconds.
    pub activity: u64,
    /// Kept once the read reached its newest entry; `None` before.
    pub stamp: Option<String>,
}

/// How far a session's transcript is indexed. Transcript reads go back
/// from the newest entry, so a read of new entries walks back from the
/// end until it reaches `done`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Progress {
    /// Entries before this index are indexed.
    pub done: u64,
    /// An unfinished walk back toward `done`.
    pub walk: Option<Walk>,
}

/// Entries `low..top` are indexed; `top` becomes `done` when the walk
/// reaches it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Walk {
    pub top: u64,
    pub low: u64,
}

impl Progress {
    pub const NEW: Self = Self {
        done: 0,
        walk: None,
    };
}

#[cfg(test)]
#[path = "../../tests/agents_search_store.rs"]
mod tests;
