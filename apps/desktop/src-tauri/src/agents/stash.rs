//! Project prompt stashes, including attachment bytes, kept on this computer.

use crate::{beep::identity::data_dir, error::Result};
use anyhow::{Context, bail};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{fs::OpenOptions, path::Path, time::Duration};
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

#[derive(Clone, Deserialize, Serialize)]
pub struct Attachment {
    name: String,
    mime: String,
    size: u64,
    data: String,
}

#[derive(Clone, Default, Deserialize, Serialize)]
pub struct Message {
    text: String,
    images: Vec<Attachment>,
    files: Vec<Attachment>,
    paths: Vec<String>,
}

#[derive(Deserialize, Serialize)]
pub struct Entry {
    id: String,
    text: String,
    images: Vec<String>,
    files: Vec<String>,
}

fn open(path: &Path) -> anyhow::Result<Connection> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?;
    let db = Connection::open(path)?;
    db.busy_timeout(Duration::from_secs(5))?;
    db.execute_batch("CREATE TABLE IF NOT EXISTS stash (id TEXT PRIMARY KEY, project TEXT NOT NULL, body TEXT NOT NULL, summary TEXT NOT NULL);
        CREATE INDEX IF NOT EXISTS stash_project ON stash(project);")?;
    Ok(db)
}

fn put(db: &Connection, project: &str, message: &Message) -> anyhow::Result<String> {
    if project.is_empty() {
        bail!("Choose a project before stashing a message");
    }
    if message.text.trim().is_empty()
        && message.images.is_empty()
        && message.files.is_empty()
        && message.paths.is_empty()
    {
        bail!("There is no message to stash");
    }
    let id = Uuid::new_v4().to_string();
    let summary = Entry {
        id: id.clone(),
        text: message.text.chars().take(1024).collect(),
        images: message
            .images
            .iter()
            .map(|file| file.name.clone())
            .collect(),
        files: message
            .files
            .iter()
            .map(|file| file.name.clone())
            .chain(message.paths.iter().cloned())
            .collect(),
    };
    db.execute(
        "INSERT INTO stash (id, project, body, summary) VALUES (?1, ?2, ?3, ?4)",
        params![
            id,
            project,
            serde_json::to_string(message)?,
            serde_json::to_string(&summary)?
        ],
    )?;
    Ok(id)
}

fn read(db: &Connection, project: &str, id: &str) -> anyhow::Result<Message> {
    let body: String = db
        .query_row(
            "SELECT body FROM stash WHERE project = ?1 AND id = ?2",
            params![project, id],
            |row| row.get(0),
        )
        .optional()?
        .context("This message is no longer in the stash")?;
    Ok(serde_json::from_str(&body)?)
}

fn take(
    db: &mut Connection,
    project: &str,
    id: &str,
    swap: Option<&Message>,
) -> anyhow::Result<Message> {
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let message = read(&tx, project, id)?;
    if let Some(swap) = swap {
        put(&tx, project, swap)?;
    }
    tx.execute(
        "DELETE FROM stash WHERE project = ?1 AND id = ?2",
        params![project, id],
    )?;
    tx.commit()?;
    Ok(message)
}

async fn run<T: Send + 'static>(
    app: AppHandle,
    project: String,
    changed: bool,
    work: impl FnOnce(&mut Connection, &str) -> anyhow::Result<T> + Send + 'static,
) -> Result<T> {
    let path = data_dir(&app)?.join("prompt-stash.sqlite");
    let event_project = project.clone();
    let result = tokio::task::spawn_blocking(move || work(&mut open(&path)?, &project))
        .await
        .map_err(anyhow::Error::from)??;
    if changed {
        let _ = app.emit("prompt-stash-changed", event_project);
    }
    Ok(result)
}

#[tauri::command]
pub async fn prompt_stash_list(app: AppHandle, project: String) -> Result<Vec<Entry>> {
    run(app, project, false, |db, project| {
        let mut query =
            db.prepare("SELECT summary FROM stash WHERE project = ?1 ORDER BY rowid DESC")?;
        query
            .query_map([project], |row| row.get::<_, String>(0))?
            .map(|row| Ok(serde_json::from_str(&row?)?))
            .collect()
    })
    .await
}

#[tauri::command]
pub async fn prompt_stash_put(app: AppHandle, project: String, message: Message) -> Result<String> {
    run(app, project, true, move |db, project| {
        put(db, project, &message)
    })
    .await
}

#[tauri::command]
pub async fn prompt_stash_read(app: AppHandle, project: String, id: String) -> Result<Message> {
    run(app, project, false, move |db, project| {
        read(db, project, &id)
    })
    .await
}

#[tauri::command]
pub async fn prompt_stash_take(
    app: AppHandle,
    project: String,
    id: String,
    swap: Option<Message>,
) -> Result<Message> {
    run(app, project, true, move |db, project| {
        take(db, project, &id, swap.as_ref())
    })
    .await
}

#[tauri::command]
pub async fn prompt_stash_remove(app: AppHandle, project: String, id: String) -> Result<()> {
    run(app, project, true, move |db, project| {
        db.execute(
            "DELETE FROM stash WHERE project = ?1 AND id = ?2",
            params![project, id],
        )?;
        Ok(())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(text: &str) -> Message {
        Message {
            text: text.into(),
            ..Message::default()
        }
    }

    #[test]
    fn stash_survives_reopening_with_attachment_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("stash.sqlite");
        let db = open(&path).unwrap();
        let mut original = message("Review the attached file");
        original.files.push(Attachment {
            name: "sample.bin".into(),
            mime: "application/octet-stream".into(),
            size: 3,
            data: "AAH/".into(),
        });
        let id = put(&db, "project-a", &original).unwrap();
        drop(db);
        let db = open(&path).unwrap();
        assert_eq!(read(&db, "project-a", &id).unwrap().files[0].data, "AAH/");
        assert!(read(&db, "project-b", &id).is_err());
    }

    #[test]
    fn pop_and_swap_are_atomic_and_only_succeed_once() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = open(&dir.path().join("stash.sqlite")).unwrap();
        let id = put(&db, "project", &message("stashed")).unwrap();
        assert_eq!(
            take(&mut db, "project", &id, Some(&message("draft")))
                .unwrap()
                .text,
            "stashed"
        );
        assert!(take(&mut db, "project", &id, Some(&message("duplicate"))).is_err());
        let count: u32 = db
            .query_row("SELECT COUNT(*) FROM stash", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
        let text: String = db
            .query_row(
                "SELECT json_extract(body, '$.text') FROM stash",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(text, "draft");
    }

    #[test]
    fn a_failed_swap_keeps_the_original_stash_entry() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = open(&dir.path().join("stash.sqlite")).unwrap();
        let id = put(&db, "project", &message("original")).unwrap();
        assert!(take(&mut db, "project", &id, Some(&Message::default())).is_err());
        assert_eq!(read(&db, "project", &id).unwrap().text, "original");
    }
}
