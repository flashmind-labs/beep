//! The prompt index on disk. Project deletion cascades through locations,
//! sessions and prompt text; triggers keep the substring index in step.

use super::super::places::Place;
use anyhow::{Result, bail};
use beep_proto::{ImportedProject, MachineSession};
use rusqlite::{Connection, OptionalExtension, params};
use std::{collections::BTreeSet, path::Path};

const SCHEMA: u32 = 2;

pub(super) struct Store(Connection);

pub(super) struct Found {
    pub session: MachineSession,
    pub texts: Vec<String>,
}

enum Owner {
    Place(i64),
    Outside,
    Removed,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let mut db = Connection::open(path)?;
        db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;")?;
        let version: u32 = db.pragma_query_value(None, "user_version", |row| row.get(0))?;

        if version > SCHEMA {
            bail!("the prompt index uses a newer schema");
        }

        let tx = db.transaction()?;

        match version {
            0 => {
                let legacy: bool = tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='sessions')",
                    [],
                    |row| row.get(0),
                )?;

                if legacy {
                    tx.execute_batch("ALTER TABLE sessions RENAME TO legacy_sessions;")?;
                }

                tx.execute_batch(include_str!("schema.sql"))?;

                if legacy {
                    tx.execute_batch(
                        "INSERT INTO sessions(id,stamp,data,activity) SELECT id,stamp,data,activity FROM legacy_sessions;
                         INSERT OR IGNORE INTO prompt_texts(session,text,words)
                             SELECT session,text,words FROM prompts WHERE session IN (SELECT id FROM sessions);
                         DROP TABLE prompts;
                         DROP TABLE legacy_sessions;
                         UPDATE sessions SET done=NULL WHERE stamp<>'';",
                    )?;
                }
            }
            // Sessions read whole before keep their prompts; the next read
            // starts from their newest page instead of the first.
            1 => tx.execute_batch(
                "ALTER TABLE sessions ADD COLUMN done INTEGER DEFAULT 0;
                 ALTER TABLE sessions ADD COLUMN walk_top INTEGER;
                 ALTER TABLE sessions ADD COLUMN walk_low INTEGER;
                 UPDATE sessions SET done=NULL WHERE stamp<>'';",
            )?,
            _ => tx.execute_batch(include_str!("schema.sql"))?,
        }

        tx.pragma_update(None, "user_version", SCHEMA)?;
        tx.commit()?;
        Ok(Self(db))
    }

    /// Whether session `id` was fully indexed at `stamp`, or was removed.
    pub fn current(&self, id: &str, stamp: &str) -> Result<bool> {
        if self.removed(id)? {
            return Ok(true);
        }

        let saved: Option<bool> = self
            .0
            .query_row(
                "SELECT stamp=?2 AND walk_top IS NULL FROM sessions WHERE id=?1",
                [id, stamp],
                |row| row.get(0),
            )
            .optional()?;
        Ok(saved == Some(true))
    }

    /// How far session `id`'s transcript is indexed.
    pub fn progress(&self, id: &str) -> Result<Progress> {
        let saved = self
            .0
            .query_row(
                "SELECT done,walk_top,walk_low FROM sessions WHERE id=?1",
                [id],
                |row| {
                    Ok((
                        row.get::<_, Option<i64>>(0)?,
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
            done: done.map(u64::try_from).transpose()?,
            walk,
        })
    }

    /// Projects come only from the authoritative imported-project list.
    /// Session scans cannot create them, so a late scan cannot undo deletion.
    pub fn sync_projects(&mut self, projects: &[ImportedProject]) -> Result<Vec<Place>> {
        let wanted: BTreeSet<Place> = projects.iter().map(Place::from).collect();
        let existing: BTreeSet<Place> = self.places(false)?.into_iter().collect();

        if wanted != existing {
            let tx = self.0.unchecked_transaction()?;

            for place in existing.difference(&wanted) {
                retire(&tx, place)?;
                tx.execute(
                    "DELETE FROM places WHERE project=?1 AND machine=?2 AND folder=?3",
                    params![place.project, place.machine, place.folder],
                )?;
            }

            for place in wanted.difference(&existing) {
                tx.execute(
                    "INSERT OR IGNORE INTO projects VALUES (?1)",
                    [&place.project],
                )?;
                tx.execute(
                    "INSERT INTO places(project,machine,folder) VALUES (?1,?2,?3)",
                    params![place.project, place.machine, place.folder],
                )?;
                tx.execute(
                    "DELETE FROM retired_places WHERE project=?1 AND machine=?2 AND folder=?3",
                    params![place.project, place.machine, place.folder],
                )?;
            }

            tx.execute(
                "DELETE FROM projects WHERE name NOT IN (SELECT project FROM places)",
                [],
            )?;
            self.attach_sessions()?;
            tx.commit()?;
        }

        self.places(true)
    }

    pub fn remove_project(
        &mut self,
        project: &str,
        known: &[ImportedProject],
    ) -> Result<Vec<Place>> {
        let tx = self.0.unchecked_transaction()?;
        tx.execute("INSERT OR IGNORE INTO retired_places SELECT project,machine,folder FROM places WHERE project=?1", [project])?;

        for place in known
            .iter()
            .filter(|p| p.project == project)
            .map(Place::from)
        {
            retire(&tx, &place)?;
        }

        tx.execute("DELETE FROM projects WHERE name=?1", [project])?;
        self.attach_sessions()?;
        tx.commit()?;
        self.places(true)
    }

    fn places(&self, retired: bool) -> Result<Vec<Place>> {
        let sql = if retired {
            "SELECT project,machine,folder FROM retired_places"
        } else {
            "SELECT project,machine,folder FROM places"
        };
        let mut read = self.0.prepare(sql)?;
        Ok(read
            .query_map([], |row| {
                Ok(Place {
                    project: row.get(0)?,
                    machine: row.get(1)?,
                    folder: row.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn owner(&self, session: &MachineSession) -> Result<Owner> {
        let mut read = self.0.prepare(
            "SELECT id,folder FROM (
                 SELECT id,folder,1 AS active FROM places WHERE machine=?1
                 UNION ALL SELECT NULL,folder,0 FROM retired_places WHERE machine=?1
             ) ORDER BY length(folder) DESC, active DESC",
        )?;
        let rows = read.query_map([&session.machine_id], |row| {
            Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, String>(1)?))
        })?;

        for row in rows {
            let (id, folder) = row?;

            if super::super::projects::within(&session.cwd, &folder) {
                return Ok(id.map_or(Owner::Removed, Owner::Place));
            }
        }

        Ok(Owner::Outside)
    }

    /// Sessions indexed before their project was imported acquire its FK.
    fn attach_sessions(&self) -> Result<()> {
        let rows: Vec<(String, String)> = {
            let mut read = self.0.prepare("SELECT id,data FROM sessions")?;
            read.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?
        };

        for (id, data) in rows {
            let session = serde_json::from_str(&data)?;

            match self.owner(&session)? {
                Owner::Place(place) => {
                    self.0.execute(
                        "UPDATE sessions SET place=?2 WHERE id=?1",
                        params![id, place],
                    )?;
                }
                Owner::Removed => {
                    self.0.execute("DELETE FROM sessions WHERE id=?1", [id])?;
                }
                Owner::Outside => {}
            }
        }

        Ok(())
    }

    fn put_session(&self, session: &MachineSession, stamp: Option<&str>) -> Result<bool> {
        let id = identity(session);

        if self.removed(&id)? {
            return Ok(false);
        }

        let place = match self.owner(session)? {
            Owner::Place(id) => Some(id),
            Owner::Outside => None,
            Owner::Removed => return Ok(false),
        };
        self.0.execute(
            "INSERT INTO sessions(id,stamp,data,activity,place) VALUES (?1,COALESCE(?2,''),?3,?4,?5)
             ON CONFLICT(id) DO UPDATE SET stamp=COALESCE(?2,sessions.stamp),data=excluded.data,
                 activity=MAX(sessions.activity,excluded.activity),place=excluded.place",
            params![id,stamp,serde_json::to_string(session)?,i64::try_from(crate::beep::epoch_ms(session.last_activity))?,place],
        )?;
        Ok(true)
    }

    /// A successfully accepted prompt is searchable before this returns.
    /// Pending rows survive a transcript snapshot taken before the send.
    pub fn append(&mut self, session: &MachineSession, text: &str) -> Result<bool> {
        let tx = self.0.unchecked_transaction()?;

        if !self.put_session(session, None)? {
            return Ok(false);
        }

        tx.execute(
            "INSERT INTO prompt_texts(session,text,words,pending) VALUES (?1,?2,?3,1)
             ON CONFLICT(session,text) DO UPDATE SET pending=1",
            params![identity(session), text, text.to_lowercase()],
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// Saves one transcript page's prompts with how far the session is
    /// indexed after it, so an interrupted read resumes from there. The
    /// stamp is kept only when the read reached the newest entry and what
    /// it had already indexed: `current`.
    pub fn save(
        &mut self,
        session: &MachineSession,
        texts: &[String],
        progress: Progress,
        current: bool,
    ) -> Result<()> {
        let tx = self.0.unchecked_transaction()?;
        let finished = current.then(|| stamp(session)).transpose()?;

        if !self.put_session(session, finished.as_deref())? {
            return Ok(());
        }

        let id = identity(session);
        let (top, low) = progress.walk.map(|walk| (walk.top, walk.low)).unzip();
        let column = |at: Option<u64>| at.map(i64::try_from).transpose();
        tx.execute(
            "UPDATE sessions SET done=?2,walk_top=?3,walk_low=?4 WHERE id=?1",
            params![id, column(progress.done)?, column(top)?, column(low)?],
        )?;
        {
            let mut insert = tx.prepare(
                "INSERT INTO prompt_texts(session,text,words) VALUES (?1,?2,?3)
                 ON CONFLICT(session,text) DO UPDATE SET pending=0",
            )?;

            for text in texts {
                insert.execute(params![id, text, text.to_lowercase()])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn removed(&self, id: &str) -> Result<bool> {
        Ok(self.0.query_row(
            "SELECT EXISTS(SELECT 1 FROM removed WHERE id=?1)",
            [id],
            |row| row.get(0),
        )?)
    }

    pub fn remove(&mut self, id: &str) -> Result<()> {
        let tx = self.0.unchecked_transaction()?;
        tx.execute("INSERT OR IGNORE INTO removed VALUES (?1)", [id])?;
        tx.execute("DELETE FROM sessions WHERE id=?1", [id])?;
        tx.commit()?;
        Ok(())
    }

    pub fn find(&self, query: &str, machines: Option<&[String]>) -> Result<Vec<Found>> {
        let query = query.trim().to_lowercase();

        if query.chars().count() < 2 {
            return Ok(Vec::new());
        }

        let terms: BTreeSet<_> = query.split_whitespace().collect();
        let mut needles = Vec::new();
        let mut conditions = Vec::new();

        for term in &terms {
            let parameter = needles.len() + 1;
            let condition = if term.chars().count() < 3 {
                needles.push(term.to_string());
                format!("instr(words, ?{parameter})>0")
            } else {
                needles.push(format!("\"{}\"", term.replace('"', "\"\"")));
                format!("id IN (SELECT rowid FROM prompt_search WHERE words MATCH ?{parameter})")
            };
            conditions.push(format!(
                "sessions.id IN (SELECT session FROM prompt_texts WHERE {condition})"
            ));
        }

        let machine_parameter = needles.len() + 1;
        let machines = machines.map(serde_json::to_string).transpose()?;
        let sql = format!(
            "SELECT sessions.id,sessions.data FROM sessions
             WHERE {} AND (?{machine_parameter} IS NULL OR json_extract(sessions.data,'$.machine_id') IN (SELECT value FROM json_each(?{machine_parameter})))
             ORDER BY sessions.activity DESC,sessions.id LIMIT 64",
            conditions.join(" AND ")
        );
        let mut parameters: Vec<Option<String>> = needles.into_iter().map(Some).collect();
        parameters.push(machines);
        let mut read = self.0.prepare(&sql)?;
        let rows = read.query_map(rusqlite::params_from_iter(parameters), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut excerpts = self.0.prepare(
            "SELECT text FROM prompt_texts WHERE session=?1 AND instr(words,?2)>0 ORDER BY id LIMIT 1"
        )?;
        rows.map(|row| {
            let (id, data) = row?;
            let texts = terms
                .iter()
                .map(|term| excerpts.query_row(params![id, term], |row| row.get(0)))
                .collect::<rusqlite::Result<Vec<String>>>()?;
            Ok(Found {
                session: serde_json::from_str(&data)?,
                texts,
            })
        })
        .collect()
    }
}

fn retire(db: &Connection, place: &Place) -> Result<()> {
    db.execute(
        "INSERT OR IGNORE INTO retired_places VALUES (?1,?2,?3)",
        params![place.project, place.machine, place.folder],
    )?;
    Ok(())
}

pub(super) fn identity(session: &MachineSession) -> String {
    format!("{} {}", session.machine_id, session.id)
}
/// A session's stamp changes when it has new activity. Stamps of schema 1
/// also held the title, so those sessions read their newest page once.
pub(super) fn stamp(session: &MachineSession) -> Result<String> {
    Ok(serde_json::to_string(&session.last_activity)?)
}

/// How far a session's transcript is indexed. Transcript reads go back
/// from the newest entry, so a read of new entries walks back from the
/// end until it reaches `done`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Progress {
    /// Entries before this index are indexed. `None` for a session the
    /// full reader of schema 1 indexed without keeping the count.
    pub done: Option<u64>,
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
        done: Some(0),
        walk: None,
    };
}

#[cfg(test)]
#[path = "../../tests/palette_prompts_store.rs"]
mod tests;
