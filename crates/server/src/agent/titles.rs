//! Names each session from its first message, then from the whole chat once quiet. Typed titles are kept.

use crate::{
    app::App,
    memory::starters::Generator,
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

/// Retitles every session that changed and went quiet; returns the retitled ones.
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
#[path = "../tests/agent_titles.rs"]
mod tests;
