//! Starters are kept 12 hours, until one's event is over or the phone changes city.

use super::User;
use crate::{
    protocol::{PromptSuggestion, PromptSuggestions},
    starters::{
        Cached, FALLBACK_FRESH, FRESH, Generator, STARTER_REFRESH_WINDOW, STARTER_REFRESHES,
        StarterContext, TIMEOUT, fallback,
    },
    store::Location,
};
use chrono::Utc;
use std::{sync::PoisonError, time::Instant};

impl User {
    /// Drops the cached starters, so the next ones see what just changed.
    pub async fn forget_starters(&self) {
        *self.starters.lock().await = None;
    }

    /// Drops the cached starters when `message` is one of them.
    pub async fn forget_used_starter(&self, message: &str) {
        let mut cache = self.starters.lock().await;
        let used = cache.as_ref().is_some_and(|cached| {
            cached
                .suggestions
                .suggestions
                .iter()
                .any(|starter| starter.prompt.trim() == message.trim())
        });
        if used {
            *cache = None;
        }
    }

    /// Counts a request for new starters; false past `STARTER_REFRESHES` per window.
    pub fn may_refresh_starters(&self) -> bool {
        let mut refreshes = self
            .starter_refreshes
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let now = Instant::now();
        while refreshes
            .front()
            .is_some_and(|at| now.duration_since(*at) >= STARTER_REFRESH_WINDOW)
        {
            refreshes.pop_front();
        }
        if refreshes.len() >= STARTER_REFRESHES {
            return false;
        }
        refreshes.push_back(now);
        true
    }

    /// The cached starters, if they still fit a phone at `here`.
    pub async fn cached_starters(&self, here: Option<&Location>) -> Option<PromptSuggestions> {
        let cache = self.starters.lock().await;
        let entry = cache.as_ref().filter(|c| c.fresh(here))?;
        Some(entry.suggestions.clone())
    }

    /// The personal starters last made, fresh or not.
    pub async fn shown_starters(&self) -> Vec<PromptSuggestion> {
        let cache = self.starters.lock().await;
        cache
            .as_ref()
            .filter(|c| c.suggestions.personal)
            .map(|c| c.suggestions.suggestions.clone())
            .unwrap_or_default()
    }

    /// Asks the model, falling back on any failure, and caches the result.
    pub async fn generate_starters(
        &self,
        generator: Generator<'_>,
        context: impl Future<Output = StarterContext>,
        here: Option<Location>,
    ) -> PromptSuggestions {
        let attempt = async {
            let context = context.await;
            generator.ask(&context).await
        };
        let (suggestions, fresh) = match tokio::time::timeout(TIMEOUT, attempt).await {
            Ok(Ok(starters)) => {
                let until_stale = starters
                    .stale_at
                    .and_then(|at| (at - Utc::now()).to_std().ok())
                    .unwrap_or(FRESH);
                (starters.suggestions, Some(until_stale.min(FRESH)))
            }
            Ok(Err(error)) => {
                log::info!("prompt suggestions fell back: {error}");
                (fallback(), None)
            }
            Err(_) => {
                log::info!("prompt suggestions fell back: timed out");
                (fallback(), None)
            }
        };
        let personal = fresh.is_some();
        let fresh = fresh.unwrap_or(FALLBACK_FRESH);
        let suggestions = PromptSuggestions {
            suggestions,
            personal,
        };
        *self.starters.lock().await = Some(Cached {
            expires: Instant::now() + fresh,
            suggestions: suggestions.clone(),
            location: here,
        });
        suggestions
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::TestDb;

    #[tokio::test]
    async fn starters_refresh_at_most_three_times_in_ten_minutes() {
        let db = TestDb::new().await.unwrap();
        let user = User::for_test(db.store().await.unwrap(), "alice");
        for _ in 0..STARTER_REFRESHES {
            assert!(user.may_refresh_starters());
        }
        assert!(!user.may_refresh_starters());
    }
}
