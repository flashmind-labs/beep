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
    #[serde(default)]
    pub pinned: bool,
}

impl Chat {
    /// A chat with no messages yet.
    pub fn new(id: Uuid, model: String, now_ms: u64) -> Self {
        Self {
            id,
            title: String::new(),
            model,
            created_ms: now_ms,
            updated_ms: now_ms,
            messages: Vec::new(),
            pinned: false,
        }
    }

    /// Takes back the `index`th prompt and everything after it, and gives its text.
    pub fn rewind(&mut self, index: usize) -> Option<String> {
        let at = self
            .messages
            .iter()
            .enumerate()
            .filter(|(_, message)| matches!(message, Message::User { .. }))
            .nth(index)?
            .0;
        let Message::User { text } = self.messages.remove(at) else {
            return None;
        };
        self.messages.truncate(at);
        Some(text)
    }
}

/// A chat as the list shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChatSummary {
    pub id: Uuid,
    pub title: String,
    pub model: String,
    pub updated_ms: u64,
    pub pinned: bool,
}

/// A chat that has the words searched for.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChatMatch {
    pub chat: ChatSummary,
    /// The line with the match, cut short.
    pub snippet: String,
    /// The user wrote the matching message.
    pub from_user: bool,
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

pub(crate) async fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
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
                pinned: chat.pinned,
            }),
            Err(e) => log::warn!("skipping assistant chat {id}: {e}"),
        }
    }
    chats.sort_by_key(|chat| std::cmp::Reverse(chat.updated_ms));
    Ok(chats)
}

/// Sets a chat's title.
pub async fn rename_chat(dir: &Path, id: Uuid, title: &str) -> Result<(), AssistantError> {
    let mut chat = load_chat(dir, id).await?;
    chat.title = title.trim().to_owned();
    save_chat(dir, &chat).await
}

/// Pins or unpins a chat.
pub async fn pin_chat(dir: &Path, id: Uuid, pinned: bool) -> Result<(), AssistantError> {
    let mut chat = load_chat(dir, id).await?;
    chat.pinned = pinned;
    save_chat(dir, &chat).await
}

const SNIPPET_CHARS: usize = 120;

/// The chats whose title or messages have every word of `query`, newest first, each with the
/// latest line that matches.
pub async fn search_chats(dir: &Path, query: &str) -> Result<Vec<ChatMatch>, AssistantError> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return Ok(Vec::new());
    }
    let has = |text: &str| words.iter().any(|word| text.to_lowercase().contains(word));
    let mut found = Vec::new();
    for summary in list_chats(dir).await? {
        let Ok(chat) = load_chat(dir, summary.id).await else {
            continue;
        };
        let texts: Vec<(&str, bool)> = chat
            .messages
            .iter()
            .filter_map(|message| match message {
                Message::User { text } => Some((text.as_str(), true)),
                Message::Assistant { text, .. } if !text.is_empty() => Some((text.as_str(), false)),
                _ => None,
            })
            .collect();
        let all = format!(
            "{} {}",
            chat.title,
            texts
                .iter()
                .map(|(text, _)| *text)
                .collect::<Vec<_>>()
                .join(" ")
        )
        .to_lowercase();
        if !words.iter().all(|word| all.contains(word)) {
            continue;
        }
        let (snippet, from_user) = match texts.iter().rev().find(|(text, _)| has(text)) {
            Some((text, from_user)) => {
                let line = text.lines().find(|line| has(line)).unwrap_or(text);
                (
                    line.trim().chars().take(SNIPPET_CHARS).collect(),
                    *from_user,
                )
            }
            None => (String::new(), false),
        };
        found.push(ChatMatch {
            chat: summary,
            snippet,
            from_user,
        });
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chat(title: &str, updated_ms: u64) -> Chat {
        Chat {
            title: title.into(),
            updated_ms,
            messages: vec![Message::User { text: "hi".into() }],
            ..Chat::new(Uuid::new_v4(), "vendor/model".into(), 1)
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

    #[tokio::test]
    async fn rename_pin_and_search() {
        let dir = tempfile::tempdir().unwrap();
        let mut one = chat("Plans", 5);
        one.messages.push(Message::Assistant {
            text: "The tokio release\nhas a task dump".into(),
            reasoning: String::new(),
            calls: vec![],
        });
        save_chat(dir.path(), &one).await.unwrap();
        save_chat(dir.path(), &chat("Other", 6)).await.unwrap();
        rename_chat(dir.path(), one.id, "  Tokio notes ")
            .await
            .unwrap();
        pin_chat(dir.path(), one.id, true).await.unwrap();
        let loaded = load_chat(dir.path(), one.id).await.unwrap();
        assert_eq!(
            (loaded.title.as_str(), loaded.pinned),
            ("Tokio notes", true)
        );
        let hits = search_chats(dir.path(), "TASK dump").await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].snippet, "has a task dump");
        assert!(!hits[0].from_user);
        assert!(
            search_chats(dir.path(), "nothing")
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn rewind_takes_back_a_prompt_and_what_followed() {
        let mut one = chat("t", 1);
        one.messages = vec![
            Message::User { text: "a".into() },
            Message::Assistant {
                text: "A".into(),
                reasoning: String::new(),
                calls: vec![],
            },
            Message::User { text: "b".into() },
            Message::Assistant {
                text: "B".into(),
                reasoning: String::new(),
                calls: vec![],
            },
        ];
        assert_eq!(one.rewind(1).as_deref(), Some("b"));
        assert_eq!(one.messages.len(), 2);
        assert_eq!(one.rewind(5), None);
    }
}
