//! Names each session: from its first message as soon as it is sent, then
//! again from the whole chat once it has been quiet for a couple of minutes.
//! A title the user typed is never replaced.

use crate::{
    app::App,
    starters::Generator,
    store::{SessionRef, Store, StoredBody, StoredMessage},
};
use anyhow::{Result, bail};
use flashmind::types::{AgentLlmConfig, LlmProvider};
use std::{sync::Arc, time::Duration};

/// A session is retitled once nobody has used it for this long.
pub const IDLE_BEFORE_RETITLE: Duration = Duration::from_secs(2 * 60);
const SWEEP_EVERY: Duration = Duration::from_secs(30);
const MAX_TITLE_CHARS: usize = 60;
/// How much of a chat the retitle reads, and of each message in it.
const MAX_CHAT_CHARS: usize = 6_000;
const MAX_MESSAGE_CHARS: usize = 600;

const SYSTEM: &str = "You name chats in Beep, a personal assistant app, for the user's chat list. \
Reply with only a title of 2 to 6 words that says what the chat is about, in the language the user wrote in. \
Sentence case, no quotes, no trailing punctuation, no emoji, and never words like \"chat\" or \"conversation\".";

/// Checks for sessions to retitle every 30 seconds.
pub fn start(app: Arc<App>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(SWEEP_EVERY);
        loop {
            interval.tick().await;
            app.retitle_idle_sessions().await;
        }
    });
}

/// Titles a session from its first message. Returns whether it got one.
pub async fn name_first(
    store: &Store,
    generator: Generator<'_>,
    session: &SessionRef,
    message: &str,
) -> bool {
    let prompt = format!("First message:\n{}", clip(message, MAX_MESSAGE_CHARS));
    match ask(&generator, prompt).await {
        Ok(title) => match store.set_generated_title(session, &title).await {
            Ok(titled) => titled,
            Err(error) => {
                log::warn!("unable to save title: {error}; session={}", session.id);
                false
            }
        },
        Err(error) => {
            log::warn!("first title failed: {error}; session={}", session.id);
            false
        }
    }
}

/// Retitles every session that changed and then went quiet, and returns
/// the ones that got a new title.
pub async fn retitle_idle(
    store: &Store,
    provider: &dyn LlmProvider,
    llm: &AgentLlmConfig,
) -> Vec<SessionRef> {
    let sessions = match store.sessions_to_title(IDLE_BEFORE_RETITLE).await {
        Ok(sessions) => sessions,
        Err(error) => {
            log::warn!("unable to list sessions to title: {error}");
            return Vec::new();
        }
    };
    let mut retitled = Vec::new();
    for session in sessions {
        let generator = Generator {
            provider,
            llm,
            user: &session.user,
        };
        match retitle(store, generator, &session).await {
            Ok(true) => retitled.push(session),
            Ok(false) => {}
            Err(error) => log::warn!("retitle failed: {error}; session={}", session.id),
        }
    }
    retitled
}

/// Returns whether the session got the new title.
async fn retitle(store: &Store, generator: Generator<'_>, session: &SessionRef) -> Result<bool> {
    let chat = store.chat_messages(session.id).await?;
    let text = render(&chat);
    if text.is_empty() {
        return Ok(false);
    }
    let title = ask(&generator, format!("Chat:\n{text}")).await?;
    store.set_generated_title(session, &title).await
}

async fn ask(generator: &Generator<'_>, prompt: String) -> Result<String> {
    match clean(&generator.text(SYSTEM, prompt).await?) {
        Some(title) => Ok(title),
        None => bail!("model wrote no usable title"),
    }
}

/// The chat's user messages and replies, oldest first, cut to fit.
fn render(chat: &[StoredMessage]) -> String {
    let mut text = String::new();
    for message in chat {
        let (speaker, said) = match &message.body {
            StoredBody::User { content, .. } => ("User", content),
            StoredBody::Assistant { content } => ("Beep", content),
            _ => continue,
        };
        let line = format!("{speaker}: {}\n", clip(said, MAX_MESSAGE_CHARS));
        if text.len() + line.len() > MAX_CHAT_CHARS {
            break;
        }
        text.push_str(&line);
    }
    text
}

/// The first line of the answer without quotes, Markdown or a trailing period.
fn clean(answer: &str) -> Option<String> {
    let line = answer
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())?;
    let decoration =
        |c: char| c == '"' || c == '\'' || c == '*' || c == '#' || c == '`' || c.is_whitespace();
    let line = line.trim_start_matches(decoration);
    let line = line.strip_prefix("Title:").unwrap_or(line);
    let line = line.trim_matches(decoration).trim_end_matches(['.', '!']);
    let title = clip(line, MAX_TITLE_CHARS);
    (!title.is_empty()).then_some(title)
}

fn clip(text: &str, chars: usize) -> String {
    text.chars()
        .take(chars)
        .collect::<String>()
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{TestDb, test_chat, test_turn};
    use crate::user::{User, UserKey};
    use uuid::Uuid;

    #[test]
    fn cleans_the_answer() {
        assert_eq!(
            clean("\n\"Trip to Singapore.\"\n").as_deref(),
            Some("Trip to Singapore")
        );
        assert_eq!(
            clean("**Title:** Dinner ideas").as_deref(),
            Some("Dinner ideas")
        );
        assert_eq!(clean("  \n "), None);
    }

    async fn chat(store: &Store, name: &str) -> SessionRef {
        let session = test_chat(name);
        test_turn(store, &UserKey::for_test("alice"), session, "hi", 1000)
            .await
            .unwrap();
        SessionRef {
            user: "alice".into(),
            id: session,
        }
    }

    async fn set_updated(store: &Store, session: Uuid, ago: &str) {
        sqlx::query("UPDATE chats SET updated_at = now() - $1::interval WHERE id = $2")
            .bind(ago)
            .bind(session)
            .execute(store.pool())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn picks_quiet_changed_sessions_and_keeps_manual_titles() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let quiet = chat(&store, "quiet").await;
        let busy = chat(&store, "busy").await;
        let renamed = chat(&store, "renamed").await;
        let old = chat(&store, "old").await;
        set_updated(&store, quiet.id, "5 minutes").await;
        set_updated(&store, renamed.id, "5 minutes").await;
        set_updated(&store, old.id, "2 days").await;
        assert!(
            User::for_test(store.clone(), "alice")
                .rename_session(renamed.id, "Mine")
                .await
                .unwrap()
        );

        let picked = store.sessions_to_title(IDLE_BEFORE_RETITLE).await.unwrap();
        assert_eq!(picked, vec![quiet.clone()]);

        // A title written after the last change is current until the next one.
        sqlx::query("UPDATE chats SET title = 'Greeting', titled_at = now() WHERE id = $1")
            .bind(quiet.id)
            .execute(store.pool())
            .await
            .unwrap();
        assert!(
            store
                .sessions_to_title(IDLE_BEFORE_RETITLE)
                .await
                .unwrap()
                .is_empty()
        );

        assert!(
            !store
                .set_generated_title(&renamed, "Generated")
                .await
                .unwrap()
        );
        assert!(store.set_generated_title(&busy, "Busy one").await.unwrap());
        let titles: Vec<(String, String)> = User::for_test(store.clone(), "alice")
            .sessions()
            .await
            .unwrap()
            .into_iter()
            .map(|s| (s.id, s.title))
            .collect();
        assert!(titles.contains(&(renamed.id.to_string(), "Mine".into())));
        assert!(titles.contains(&(busy.id.to_string(), "Busy one".into())));
        assert!(titles.contains(&(quiet.id.to_string(), "Greeting".into())));
        assert!(titles.contains(&(old.id.to_string(), "hi".into())));
    }

    #[tokio::test]
    async fn deletes_sessions_in_bulk() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        for id in ["a", "b", "c"] {
            chat(&store, id).await;
        }
        let deleted = User::for_test(store.clone(), "alice")
            .delete_sessions(&[test_chat("a"), test_chat("c"), test_chat("missing")])
            .await
            .unwrap();
        assert_eq!(deleted, 2);
        let left: Vec<String> = User::for_test(store.clone(), "alice")
            .sessions()
            .await
            .unwrap()
            .into_iter()
            .map(|s| s.id)
            .collect();
        assert_eq!(left, vec![test_chat("b").to_string()]);
    }
}
