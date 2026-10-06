//! Conversation starters for the empty chat.

use super::App;
use crate::{
    protocol::PromptSuggestions,
    starters::{LocalTime, MemoryNote, StarterContext},
    store::Location,
    user::User,
};
use chrono::Utc;
use chrono_tz::Tz;

impl App {
    /// Conversation starters for the empty chat, personalized and cached
    /// until they go stale, unless `refresh`. Never fails: falls back to fixed
    /// starters.
    pub async fn suggest_prompts(&self, user: &User, refresh: bool) -> PromptSuggestions {
        let here = user.get_location().await.ok().flatten();
        if !refresh && let Some(cached) = user.cached_starters(here.as_ref()).await {
            return cached;
        }
        let context = self.starter_context(user, here.clone());
        user.generate_starters(self.generator(user.id()), context, here)
            .await
    }

    async fn starter_context(&self, user: &User, location: Option<Location>) -> StarterContext {
        let today = user.today().await;
        let (memories, sessions, apps) =
            tokio::join!(user.memory_rows(), user.sessions(), user.apps());
        let memories = memories
            .unwrap_or_default()
            .into_iter()
            .filter(|memory| !memory.valid.ended(today))
            .map(|memory| MemoryNote {
                category: memory.category,
                content: memory.content,
            })
            .collect();
        let recent_titles = sessions
            .unwrap_or_default()
            .into_iter()
            .map(|session| session.title)
            .collect();
        let timezone = location
            .as_ref()
            .and_then(|location| location.timezone.parse::<Tz>().ok());
        let place = location.and_then(|location| location.place);
        let connected_apps = apps
            .unwrap_or_default()
            .into_iter()
            .filter(|app| app.connected)
            .map(|app| app.name)
            .collect();
        StarterContext {
            memories,
            recent_titles,
            now: LocalTime {
                time: Utc::now(),
                timezone,
            },
            place,
            connected_apps,
        }
    }
}
