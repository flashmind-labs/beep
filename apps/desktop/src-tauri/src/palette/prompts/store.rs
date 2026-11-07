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
mod tests {
    use super::*;
    use beep_proto::AgentStatus;
    use std::time::{Duration, UNIX_EPOCH};

    fn session(id: &str, machine: &str, activity: u64) -> MachineSession {
        MachineSession {
            id: id.into(),
            machine_id: machine.into(),
            machine_name: machine.into(),
            agent: "codex".into(),
            title: Some("Connection fixes".into()),
            cwd: "/srv/project".into(),
            status: AgentStatus::Ended {},
            doing: None,
            started_at: UNIX_EPOCH,
            last_activity: UNIX_EPOCH + Duration::from_secs(activity),
            external: false,
            question: None,
        }
    }

    #[test]
    fn prompt_substrings_survive_restart_and_stay_per_machine() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("search.sqlite");
        {
            let mut store = Store::open(&path)?;
            store.replace(
                &session("a", "build-box", 1),
                &["Please add a retry button".into(), "Retry later".into()],
                true,
            )?;
            store.replace(
                &session("a", "gpu-box", 2),
                &["RETRY the upload".into()],
                true,
            )?;
        }
        let store = Store::open(&path)?;
        let found = store.find("  ReTrY ", None)?;
        assert_eq!(
            found.len(),
            2,
            "one hit per session, including identical ids on different machines"
        );
        assert_eq!(found[0].session.machine_id, "gpu-box");
        assert_eq!(
            store.find("try but", None)?.len(),
            1,
            "substring across word boundaries"
        );
        assert_eq!(store.find("xy", None)?.len(), 0);
        assert_eq!(store.find("tr", None)?.len(), 2);
        let members = vec!["build-box".to_owned()];
        let found = store.find("retry", Some(&members))?;
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].session.machine_id, "build-box");
        Ok(())
    }

    #[test]
    fn literal_punctuation_unicode_and_replacements() -> Result<()> {
        let mut store = Store::open(Path::new(":memory:"))?;
        let session = session("a", "build-box", 1);
        store.replace(
            &session,
            &["Find \"quoted\" text with 100% and __ plus café 東京".into()],
            true,
        )?;
        for query in ["\"quoted\"", "100%", "__", "café", "東京", "text with"] {
            assert_eq!(store.find(query, None)?.len(), 1, "{query}");
        }
        assert!(store.find("' OR 1=1", None)?.is_empty());
        assert!(store.current(&identity(&session), &stamp(&session)?)?);
        store.replace(&session, &["A different prompt".into()], true)?;
        assert!(store.find("quoted", None)?.is_empty());
        assert_eq!(store.find("different", None)?.len(), 1);
        store.remove(&identity(&session))?;
        assert!(store.find("different", None)?.is_empty());
        store.replace(&session, &["A late indexing result".into()], true)?;
        assert!(
            store.find("indexing", None)?.is_empty(),
            "deletion wins over an indexing task already in flight"
        );
        Ok(())
    }

    #[test]
    fn a_compacted_transcript_keeps_previously_indexed_prompts() -> Result<()> {
        let mut store = Store::open(Path::new(":memory:"))?;
        let session = session("a", "build-box", 1);
        store.replace(&session, &["Original request".into()], true)?;
        store.replace(&session, &["Follow up request".into()], false)?;
        assert_eq!(store.find("Original", None)?.len(), 1);
        assert_eq!(store.find("Follow up", None)?.len(), 1);
        Ok(())
    }

    fn project(name: &str, machine: &str, folder: &str) -> ImportedProject {
        ImportedProject {
            project: name.into(),
            machine_id: machine.into(),
            machine_name: machine.into(),
            folder: folder.into(),
            url: format!("https://example.com/{name}"),
            cloned: false,
            imported_at: UNIX_EPOCH,
        }
    }

    fn count(store: &Store, table: &str) -> Result<i64> {
        Ok(store
            .0
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                row.get(0)
            })?)
    }

    #[test]
    fn accepted_prompts_are_immediate_durable_and_survive_an_older_scan() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("search.sqlite");
        let session = session("a", "build-box", 1);
        {
            let mut writer = Store::open(&path)?;
            let reader = Store::open(&path)?;
            writer.append(&session, "Add a retry button")?;
            assert_eq!(reader.find("retry", None)?.len(), 1);
            writer.replace(&session, &["Earlier request".into()], true)?;
            assert_eq!(
                reader.find("retry", None)?.len(),
                1,
                "a scan taken before the send cannot erase it"
            );
            writer.append(&session, "Add a retry button")?;
            writer.replace(
                &session,
                &["Earlier request".into(), "Add a retry button".into()],
                true,
            )?;
            assert_eq!(
                count(&writer, "prompt_texts")?,
                2,
                "the reconciled prompt is not duplicated"
            );
        }
        assert_eq!(Store::open(&path)?.find("retry", None)?.len(), 1);
        Ok(())
    }

    #[test]
    fn project_deletion_cascades_and_stale_scans_cannot_restore_it() -> Result<()> {
        let mut store = Store::open(Path::new(":memory:"))?;
        let projects = vec![
            project("acme/widget", "build-box", "/srv/project"),
            project("acme/widget", "gpu-box", "C:\\work\\widget"),
        ];
        store.sync_projects(&projects)?;
        let build = session("a", "build-box", 1);
        let mut gpu = session("a", "gpu-box", 1);
        gpu.cwd = "C:\\work\\widget\\.worktrees\\fix".into();
        store.append(&build, "retry button")?;
        store.append(&gpu, "retry upload")?;
        store.remove_project("acme/widget", &projects)?;
        for table in ["projects", "places", "sessions", "prompt_texts"] {
            assert_eq!(count(&store, table)?, 0, "{table}");
        }
        assert!(
            store.find("retry", None)?.is_empty(),
            "FTS entries are deleted too"
        );
        store.replace(&build, &["a stale retry result".into()], true)?;
        assert!(!store.append(&gpu, "retry again")?);
        let new = session("never-indexed", "build-box", 2);
        store.replace(&new, &["new stale retry".into()], true)?;
        assert!(store.find("retry", None)?.is_empty());
        let violations: i64 =
            store
                .0
                .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
                    row.get(0)
                })?;
        assert_eq!(violations, 0);
        assert!(store.0.execute("INSERT INTO prompt_texts(session,text,words) VALUES('missing','orphan','orphan')", []).is_err());
        assert!(
            store
                .0
                .execute("INSERT INTO sessions VALUES('orphan','','{}',0,999)", [])
                .is_err()
        );
        store.0.execute(
            "INSERT INTO prompt_search(prompt_search) VALUES('integrity-check')",
            [],
        )?;
        store.sync_projects(&projects)?;
        assert!(
            store.append(&build, "retry after reimport")?,
            "an explicit reimport makes the location searchable again"
        );
        Ok(())
    }

    #[test]
    fn removing_one_location_preserves_the_other_and_unrelated_folders() -> Result<()> {
        let mut store = Store::open(Path::new(":memory:"))?;
        let projects = vec![
            project("acme/widget", "build-box", "/srv/project"),
            project("acme/widget", "gpu-box", "/srv/project"),
        ];
        let build = session("a", "build-box", 1);
        let gpu = session("a", "gpu-box", 1);
        let mut other = session("other", "build-box", 1);
        other.cwd = "/srv/project-other".into();
        store.append(&build, "retry build")?;
        store.append(&gpu, "retry upload")?;
        store.append(&other, "retry elsewhere")?;
        store.sync_projects(&projects)?;
        store.sync_projects(&projects[1..])?;
        let found = store.find("retry", None)?;
        assert_eq!(found.len(), 2);
        assert!(
            found
                .iter()
                .all(|f| f.session.machine_id == "gpu-box" || f.session.id == "other")
        );
        assert!(!store.append(&build, "late retry")?);
        Ok(())
    }

    #[test]
    fn a_removed_nested_project_does_not_fall_back_to_its_parent() -> Result<()> {
        let mut store = Store::open(Path::new(":memory:"))?;
        let projects = vec![
            project("acme/parent", "build-box", "/srv"),
            project("acme/widget", "build-box", "/srv/project"),
        ];
        store.sync_projects(&projects)?;
        let child = session("a", "build-box", 1);
        store.append(&child, "nested request")?;
        store.sync_projects(&projects[..1])?;
        assert!(!store.append(&child, "must stay removed")?);
        assert_eq!(count(&store, "projects")?, 1);
        assert_eq!(count(&store, "sessions")?, 0);
        Ok(())
    }

    #[test]
    fn the_previous_disk_index_is_migrated_and_acquires_project_ownership() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("search.sqlite");
        let session = session("a", "build-box", 1);
        {
            let db = Connection::open(&path)?;
            db.execute_batch("CREATE TABLE sessions(id TEXT PRIMARY KEY,stamp TEXT,data TEXT,activity INTEGER); CREATE TABLE removed(id TEXT PRIMARY KEY); CREATE VIRTUAL TABLE prompts USING fts5(session UNINDEXED,text UNINDEXED,words,tokenize='trigram');")?;
            db.execute(
                "INSERT INTO sessions VALUES(?1,?2,?3,0)",
                params![
                    identity(&session),
                    stamp(&session)?,
                    serde_json::to_string(&session)?
                ],
            )?;
            db.execute(
                "INSERT INTO prompts VALUES(?1,'Original prompt','original prompt')",
                [identity(&session)],
            )?;
        }
        let mut store = Store::open(&path)?;
        assert_eq!(store.find("original", None)?.len(), 1);
        let projects = vec![project("acme/widget", "build-box", "/srv/project")];
        store.sync_projects(&projects)?;
        store.remove_project("acme/widget", &projects)?;
        assert!(store.find("original", None)?.is_empty());
        Ok(())
    }

    #[test]
    fn unchanged_empty_sessions_are_also_indexed() -> Result<()> {
        let mut store = Store::open(Path::new(":memory:"))?;
        let session = session("a", "build-box", 1);
        store.replace(&session, &[], true)?;
        assert!(store.current(&identity(&session), &stamp(&session)?)?);
        Ok(())
    }
}
