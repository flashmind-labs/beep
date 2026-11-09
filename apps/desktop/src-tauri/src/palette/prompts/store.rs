//! The prompt index on disk. Project deletion cascades through locations,
//! sessions and prompt text; triggers keep the substring index in step.

use super::super::places::Place;
use anyhow::{Result, bail};
use beep_proto::{ImportedProject, MachineSession};
use rusqlite::{Connection, OptionalExtension, params};
use std::{collections::BTreeSet, path::Path};

pub(super) struct Store(Connection);

pub(super) struct Found {
    pub session: MachineSession,
    pub text: String,
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

        if version > 1 {
            bail!("the prompt index uses a newer schema");
        }

        if version == 0 {
            let legacy: bool = db.query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='sessions')",
                [],
                |row| row.get(0),
            )?;
            let tx = db.transaction()?;

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
                     DROP TABLE legacy_sessions;",
                )?;
            }

            tx.pragma_update(None, "user_version", 1)?;
            tx.commit()?;
        } else {
            db.execute_batch(include_str!("schema.sql"))?;
        }

        Ok(Self(db))
    }

    pub fn current(&self, id: &str, stamp: &str) -> Result<bool> {
        if self.removed(id)? {
            return Ok(true);
        }

        let saved: Option<String> = self
            .0
            .query_row("SELECT stamp FROM sessions WHERE id=?1", [id], |row| {
                row.get(0)
            })
            .optional()?;
        Ok(saved.as_deref() == Some(stamp))
    }

    pub fn session(&self, id: &str) -> Result<Option<MachineSession>> {
        let data: Option<String> = self
            .0
            .query_row("SELECT data FROM sessions WHERE id=?1", [id], |row| {
                row.get(0)
            })
            .optional()?;
        Ok(data.map(|data| serde_json::from_str(&data)).transpose()?)
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

    pub fn replace(
        &mut self,
        session: &MachineSession,
        texts: &[String],
        complete: bool,
    ) -> Result<()> {
        let tx = self.0.unchecked_transaction()?;

        if !self.put_session(session, Some(&stamp(session)?))? {
            return Ok(());
        }

        let id = identity(session);

        if complete {
            tx.execute(
                "DELETE FROM prompt_texts WHERE session=?1 AND pending=0",
                [&id],
            )?;
        }
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

        let (condition, needle) = if query.chars().count() < 3 {
            ("instr(prompt_texts.words, ?1)>0", query)
        } else {
            (
                "prompt_texts.id IN (SELECT rowid FROM prompt_search WHERE words MATCH ?1)",
                format!("\"{}\"", query.replace('"', "\"\"")),
            )
        };
        let sql = format!(
            "SELECT sessions.data,prompt_texts.text FROM prompt_texts
             JOIN sessions ON sessions.id=prompt_texts.session
             WHERE {condition} AND (?2 IS NULL OR json_extract(sessions.data,'$.machine_id') IN (SELECT value FROM json_each(?2)))
             GROUP BY sessions.id ORDER BY sessions.activity DESC,sessions.id LIMIT 64"
        );
        let mut read = self.0.prepare(&sql)?;
        let machines = machines.map(serde_json::to_string).transpose()?;
        let rows = read.query_map(params![needle, machines], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.map(|row| {
            let (data, text) = row?;
            Ok(Found {
                session: serde_json::from_str(&data)?,
                text,
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
pub(super) fn stamp(session: &MachineSession) -> Result<String> {
    Ok(serde_json::to_string(&(
        session.last_activity,
        &session.title,
    ))?)
}
#[cfg(test)]
#[path = "../../tests/palette_prompts_store.rs"]
mod tests;
