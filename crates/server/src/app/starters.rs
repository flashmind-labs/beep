//! Conversation starters for the empty chat.

use super::App;
use crate::{
    account::user::User,
    memory::starters::{Done, LocalTime, MAX_DONE, MemoryNote, StarterContext},
    protocol::PromptSuggestions,
    store::Location,
};
use chrono::Utc;

impl App {
    /// Personalized starters for the empty chat, cached until stale; falls back to fixed ones.
    pub async fn suggest_prompts(&self, user: &User, refresh: bool) -> PromptSuggestions {
        let here = user.get_location().await.ok().flatten();
        // Past the limit a refresh gets the cached set.
        let refresh = refresh && user.may_refresh_starters();
        if !refresh && let Some(cached) = user.cached_starters(here.as_ref()).await {
            return cached;
        }
        let context = starter_context(user, here.clone(), refresh);
        user.generate_starters(self.generator(user.id()), context, here)
            .await
    }
}

/// What the model is told; a `refresh` also gets the set it replaces.
async fn starter_context(user: &User, location: Option<Location>, refresh: bool) -> StarterContext {
    let today = user.today().await;
    let shown = if refresh {
        user.shown_starters().await
    } else {
        Vec::new()
    };
    let (memories, sessions, apps, actions) = tokio::join!(
        user.memory_rows(),
        user.sessions(),
        user.apps(),
        user.recent_actions(MAX_DONE as i64)
    );
    let done = actions
        .unwrap_or_default()
        .into_iter()
        .map(|action| Done {
            at: action.done_at,
            description: action.description,
        })
        .collect();
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
    // The saved timezone, which a location also sets: the user may have
    // one without sharing where they are.
    let timezone = user.timezone().await;
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
        done,
        shown,
    }
}
