//! Chats on disk: one JSON file per chat, `<id>.json`, readable by the user only.

use crate::{AssistantError, Message};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// A chat.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chat {
    pub id: Uuid,
    pub title: String,
    /// The OpenRouter model id the chat runs on.
    pub model: String,
    /// Milliseconds since the Unix epoch.
    pub created_ms: u64,
    pub updated_ms: u64,
    pub messages: Vec<Message>,
}

/// A chat as the list shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChatSummary {
    pub id: Uuid,
    pub title: String,
    pub model: String,
    pub updated_ms: u64,
}

fn path(dir: &Path, id: Uuid) -> PathBuf {
    dir.join(format!("{id}.json"))
}

/// Writes the chat through a temporary file of its own, flushed to disk before it replaces the
/// old one, so a crash or a second writer leaves a whole file.
pub async fn save_chat(dir: &Path, chat: &Chat) -> Result<(), AssistantError> {
    tokio::fs::create_dir_all(dir).await?;
    let bytes = serde_json::to_vec(chat)?;
    let tmp = dir.join(format!("{}.{}.tmp", chat.id, Uuid::new_v4()));
    let written = write_private(&tmp, &bytes).await;
    let renamed = match written {
        Ok(()) => tokio::fs::rename(&tmp, path(dir, chat.id)).await,
        Err(e) => Err(e),
    };
    if let Err(e) = renamed {
        // The temporary file is useless now; the error that matters is the write's.
        tokio::fs::remove_file(&tmp).await.ok();
        return Err(e.into());
    }
    Ok(())
}

async fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use tokio::io::AsyncWriteExt;
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(path).await?;
    file.write_all(bytes).await?;
    file.sync_all().await
}

pub async fn load_chat(dir: &Path, id: Uuid) -> Result<Chat, AssistantError> {
    let bytes = tokio::fs::read(path(dir, id)).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub async fn delete_chat(dir: &Path, id: Uuid) -> Result<(), AssistantError> {
    match tokio::fs::remove_file(path(dir, id)).await {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// The saved chats, newest first. A file that does not read is left out, so one bad file never
/// hides the rest.
pub async fn list_chats(dir: &Path) -> Result<Vec<ChatSummary>, AssistantError> {
    let mut entries = match tokio::fs::read_dir(dir).await {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let mut chats = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        let file = entry.path();
        let Some(id) = file
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(".json"))
            .and_then(|stem| Uuid::parse_str(stem).ok())
        else {
            continue;
        };
        match load_chat(dir, id).await {
            Ok(chat) => chats.push(ChatSummary {
                id,
                title: chat.title,
                model: chat.model,
                updated_ms: chat.updated_ms,
            }),
            Err(e) => log::warn!("skipping assistant chat {id}: {e}"),
        }
    }
    chats.sort_by_key(|chat| std::cmp::Reverse(chat.updated_ms));
    Ok(chats)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chat(title: &str, updated_ms: u64) -> Chat {
        Chat {
            id: Uuid::new_v4(),
            title: title.into(),
            model: "vendor/model".into(),
            created_ms: 1,
            updated_ms,
            messages: vec![Message::User { text: "hi".into() }],
        }
    }

    #[tokio::test]
    async fn saves_reloads_lists_and_deletes() {
        let dir = tempfile::tempdir().unwrap();
        let (old, new) = (chat("old", 10), chat("new", 20));
        save_chat(dir.path(), &old).await.unwrap();
        save_chat(dir.path(), &new).await.unwrap();
        assert_eq!(load_chat(dir.path(), new.id).await.unwrap(), new);
        let titles: Vec<_> = list_chats(dir.path())
            .await
            .unwrap()
            .into_iter()
            .map(|c| c.title)
            .collect();
        assert_eq!(titles, ["new", "old"]);
        delete_chat(dir.path(), new.id).await.unwrap();
        delete_chat(dir.path(), new.id).await.unwrap();
        assert_eq!(list_chats(dir.path()).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_corrupt_file_does_not_break_the_list() {
        let dir = tempfile::tempdir().unwrap();
        let good = chat("good", 5);
        save_chat(dir.path(), &good).await.unwrap();
        tokio::fs::write(path(dir.path(), Uuid::new_v4()), b"{not json")
            .await
            .unwrap();
        let listed = list_chats(dir.path()).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, good.id);
    }

    #[tokio::test]
    async fn a_missing_folder_lists_nothing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            list_chats(&dir.path().join("none"))
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn files_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let one = chat("one", 1);
        save_chat(dir.path(), &one).await.unwrap();
        let mode = std::fs::metadata(path(dir.path(), one.id))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[tokio::test]
    async fn saving_leaves_no_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let one = chat("one", 1);
        save_chat(dir.path(), &one).await.unwrap();
        save_chat(dir.path(), &one).await.unwrap();
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(names, [format!("{}.json", one.id)]);
    }
}
