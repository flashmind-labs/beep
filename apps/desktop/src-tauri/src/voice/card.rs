//! The orb's card for a new notice, when Voice notes is set to ask first.
//! It shows who or what the notice is from and the start of its text, with
//! Play, Open and ×. Play reads it aloud as a voice note; Open, a click on
//! the card or two taps of Option open its chat in the bar. Left alone for
//! `CARD_FOR` it goes, and the notice shows as its banner instead.

use super::Voice;
use crate::{bar, orb};
use beep_proto::Notice;
use serde::Serialize;
use std::{
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};
use tauri::Emitter;
use tokio::time::sleep;
use uuid::Uuid;

/// How long the card stays untouched, counted again when the pointer
/// leaves it.
const CARD_FOR: Duration = Duration::from_secs(20);
/// The most of the notice's text the card is sent; it shows two lines.
const SNIPPET: usize = 200;

/// A notice the card offers.
pub(super) struct Pending {
    id: u64,
    notice: Notice,
    /// When it goes, unless the pointer is on it.
    until: Instant,
    held: bool,
}

/// What the orb's card shows, as the `notice-card` event.
#[derive(Debug, Clone, Serialize)]
pub struct NoticeCard {
    id: u64,
    title: String,
    snippet: String,
}

impl NoticeCard {
    fn of(id: u64, notice: &Notice) -> Self {
        let title = notice.title.trim();
        let body = notice.body.split_whitespace().collect::<Vec<_>>().join(" ");
        let snippet = match body.char_indices().nth(SNIPPET) {
            Some((end, _)) => format!("{}…", body[..end].trim_end()),
            None => body,
        };
        Self {
            id,
            title: if title.is_empty() { "Beep" } else { title }.to_owned(),
            snippet,
        }
    }
}

impl Voice {
    /// Shows `notice` on the orb's card. A card already up gives way and
    /// shows as its banner.
    pub fn offer(self: &Arc<Self>, notice: Notice) {
        let id = self.lives.fetch_add(1, Ordering::Relaxed) + 1;
        let card = NoticeCard::of(id, &notice);
        let before = self.state().card.replace(Pending {
            id,
            notice,
            until: Instant::now() + CARD_FOR,
            held: false,
        });
        if let Some(before) = before {
            crate::notify(&self.app, &before.notice);
        }
        if let Err(e) = orb::show(&self.app) {
            log::warn!("showing the orb for a notice: {e}");
        }
        self.emit_card(Some(card));
        let me = self.clone();
        tauri::async_runtime::spawn(async move { me.expire(id).await });
    }

    /// The card up now, for an orb opened after it came.
    pub fn notice_card(&self) -> Option<NoticeCard> {
        let state = self.state();
        state.card.as_ref().map(|c| NoticeCard::of(c.id, &c.notice))
    }

    /// Play: the notice is read aloud in the orb.
    pub fn play_card(self: &Arc<Self>, id: u64) {
        let Some(card) = self.take_card(Some(id)) else {
            return;
        };
        self.emit_card(None);
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            me.clone().speak_notice(card.notice).await;
            // It did not start, and its banner shows instead.
            if !me.is_live() {
                orb::hide(&me.app);
            }
        });
    }

    /// Open: the bar opens on the notice's chat.
    pub fn open_card(&self, id: u64) {
        if let Some(card) = self.take_card(Some(id)) {
            self.open_chat_of(&card);
        }
    }

    /// Opens the card up now, if there is one.
    pub(super) fn open_any_card(&self) -> bool {
        match self.take_card(None) {
            Some(card) => {
                self.open_chat_of(&card);
                true
            }
            None => false,
        }
    }

    /// ×: the card goes. The notice stays in its chat.
    pub fn dismiss_card(&self, id: u64) {
        if self.take_card(Some(id)).is_some() {
            self.emit_card(None);
            self.hide_orb_unless_live();
        }
    }

    /// The pointer came onto the card, which keeps it up, or left it, and
    /// it stays `CARD_FOR` more.
    pub fn hold_card(&self, id: u64, held: bool) {
        let mut state = self.state();
        if let Some(card) = state.card.as_mut().filter(|c| c.id == id) {
            card.held = held;
            card.until = Instant::now() + CARD_FOR;
        }
    }

    /// The card gave way without being answered: the notice shows as its
    /// banner, unless the bar already shows its chat.
    pub(super) fn card_gone(&self, card: Pending) {
        self.emit_card(None);
        let shown = bar::is_open(&self.app)
            && self.beep.session().as_deref() == Some(card.notice.session_id.as_str());
        if !shown {
            crate::notify(&self.app, &card.notice);
        }
    }

    /// Waits out card `id`, then lets it go.
    async fn expire(self: Arc<Self>, id: u64) {
        let mut wait = CARD_FOR;
        loop {
            sleep(wait).await;
            let gone = {
                let mut state = self.state();
                let Some(card) = state.card.as_ref().filter(|c| c.id == id) else {
                    return;
                };
                let now = Instant::now();
                if card.held {
                    wait = CARD_FOR;
                    None
                } else if card.until > now {
                    wait = card.until - now;
                    None
                } else {
                    state.card.take()
                }
            };
            if let Some(card) = gone {
                self.card_gone(card);
                self.hide_orb_unless_live();
                return;
            }
        }
    }

    /// Takes card `id`, or whichever card is up.
    fn take_card(&self, id: Option<u64>) -> Option<Pending> {
        let mut state = self.state();
        if state
            .card
            .as_ref()
            .is_some_and(|c| id.is_none_or(|id| c.id == id))
        {
            state.card.take()
        } else {
            None
        }
    }

    fn open_chat_of(&self, card: &Pending) {
        self.emit_card(None);
        self.hide_orb_unless_live();
        if Uuid::parse_str(&card.notice.session_id).is_ok() {
            self.beep.set_session(Some(card.notice.session_id.clone()));
        }
        bar::show(&self.app, Some("open"));
    }

    fn hide_orb_unless_live(&self) {
        if !self.is_live() {
            orb::hide(&self.app);
        }
    }

    fn emit_card(&self, card: Option<NoticeCard>) {
        if let Err(e) = self.app.emit_to(orb::LABEL, "notice-card", &card) {
            log::debug!("telling the orb about a notice: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notice(title: &str, body: &str) -> Notice {
        Notice {
            title: title.into(),
            body: body.into(),
            session_id: String::new(),
            app: None,
            approval: None,
            changed: false,
            group: false,
        }
    }

    #[test]
    fn the_card_shows_the_title_and_the_start_of_the_text() {
        let card = NoticeCard::of(1, &notice(" Inbox sweep ", "Two emails\n\nneed a reply."));
        assert_eq!(card.title, "Inbox sweep");
        assert_eq!(card.snippet, "Two emails need a reply.");
        let long = NoticeCard::of(2, &notice("", &"word ".repeat(100)));
        assert_eq!(long.title, "Beep");
        assert!(long.snippet.ends_with('…'));
        assert!(long.snippet.chars().count() <= SNIPPET + 1);
    }
}
