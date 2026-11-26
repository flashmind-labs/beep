//! Asks between the agents of a team. A session's `team_ask` goes through
//! its mesh's gateway to the teammate's gateway, which hands it to the
//! session on the card it names, or keeps it as a Needs you card for this
//! mesh's apps when it is for the person (`incoming`). The session asked
//! answers once, with `team_ask` and the ask's id, and the answer goes back
//! the same way into the asking session.

use beep_proto::{
    SessionId,
    team::{AgentRef, Card, Target, TeamRepo},
};
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    time::{Duration, Instant, SystemTime},
};

/// The longest ask or answer, in characters.
pub(crate) const MAX_TEXT: usize = 4000;
/// What a device that holds no ask by an answer's id says: the sender
/// tries the teammate's next device.
pub(super) const NOT_WAITING: &str = "No ask waits for this answer here";
/// What starts the id of an ask's Needs you card, before the ask's id.
pub(super) const ASK_CARD: &str = "ask:";
/// How many asks a session sends in an hour.
const PER_HOUR: usize = 10;
const HOUR: Duration = Duration::from_secs(60 * 60);
/// How long an ask waits for its answer, and its Needs you card stays.
const KEPT_FOR: Duration = Duration::from_secs(24 * 60 * 60);
/// The most asks waiting each way, and Needs you cards of one team.
pub(super) const MAX_WAITING: usize = 256;
pub(super) const MAX_CARDS: usize = 32;
/// The longest card id.
pub(super) const MAX_CARD: usize = 256;
/// What a device that does not act for `repo`'s team says to a board's
/// subscriber or an ask: the sender tries the teammate's next device.
pub(super) fn not_served(repo: &TeamRepo) -> String {
    format!("This device does not serve {repo}'s board")
}

/// The asks each session sent in the last hour.
#[derive(Debug, Default)]
pub(crate) struct AskLimit(HashMap<SessionId, VecDeque<Instant>>);

impl AskLimit {
    /// Counts an ask `session` sends `now`, or refuses the 11th in an hour.
    pub(crate) fn take(&mut self, session: &SessionId, now: Instant) -> Result<(), String> {
        self.0.retain(|_, sent| {
            sent.retain(|at| now.saturating_duration_since(*at) < HOUR);
            !sent.is_empty()
        });
        let sent = self.0.entry(session.clone()).or_default();

        if sent.len() >= PER_HOUR {
            return Err("team_ask is limited to 10 messages an hour".to_owned());
        }

        sent.push_back(now);
        Ok(())
    }

    /// Takes back the last ask `session` counted: it was not sent.
    pub(crate) fn give_back(&mut self, session: &SessionId) {
        if let Some(sent) = self.0.get_mut(session) {
            sent.pop_back();
        }
    }
}

/// What a text handed to a session is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Delivery {
    /// A teammate's agent asks: a message, which counts against the
    /// session's messages from agents.
    Ask,
    /// The answer to the session's own ask: a note.
    Reply,
}

/// `Tool::TeamAsk` from a session's node to its mesh's gateway: the ask as
/// that node made it, or the answer to one when `reply_to` is set, which
/// goes to whoever asked and needs no `to`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Outgoing {
    pub(crate) repo: TeamRepo,
    pub(crate) from: AgentRef,
    pub(crate) to: Option<Target>,
    pub(crate) text: String,
    pub(crate) reply_to: Option<String>,
}

/// The asks this device keeps as its mesh's gateway.
#[derive(Debug, Default)]
pub(crate) struct Asks {
    /// What this device's sessions sent, so one does not flood the team.
    pub(crate) limit: AskLimit,
    /// Sent to teammates, by id, until answered.
    pub(super) sent: BTreeMap<String, Sent>,
    /// Taken from teammates, by id, until answered once.
    pub(super) taken: BTreeMap<String, Taken>,
    /// The asks for this mesh's person, as Needs you cards for this mesh's
    /// own apps: they never go on the board teammates see.
    pub(super) cards: BTreeMap<TeamRepo, Vec<Card>>,
}

#[derive(Debug, Clone)]
pub(super) struct Sent {
    pub(super) repo: TeamRepo,
    /// The asking session's card.
    pub(super) asker: String,
    /// The mesh of the member asked, the only one who answers.
    pub(super) mesh: EndpointId,
    /// The card or login asked.
    pub(super) asked: String,
    pub(super) at: SystemTime,
}

#[derive(Debug, Clone)]
pub(super) struct Taken {
    pub(super) repo: TeamRepo,
    /// The card asked: its session answers.
    pub(super) card: String,
    /// The mesh of the member who asked.
    pub(super) mesh: EndpointId,
    pub(super) at: SystemTime,
}

impl Asks {
    /// The Needs you cards of `repo`'s asks.
    pub(crate) fn cards(&mut self, repo: &TeamRepo, now: SystemTime) -> Vec<Card> {
        self.expire(now);
        self.cards.get(repo).cloned().unwrap_or_default()
    }

    /// When the oldest ask or card expires.
    pub(super) fn due(&self) -> Option<SystemTime> {
        let cards = self.cards.values().flatten().map(|card| card.updated_at);

        self.sent
            .values()
            .map(|sent| sent.at)
            .chain(self.taken.values().map(|taken| taken.at))
            .chain(cards)
            .map(|at| at + KEPT_FOR)
            .min()
    }

    pub(super) fn expire(&mut self, now: SystemTime) {
        let fresh = |at: SystemTime| now.duration_since(at).map_or(true, |age| age < KEPT_FOR);
        self.sent.retain(|_, sent| fresh(sent.at));
        self.taken.retain(|_, taken| fresh(taken.at));
        self.cards.retain(|_, cards| {
            cards.retain(|card| fresh(card.updated_at));
            !cards.is_empty()
        });
    }
}

/// A new ask's id: 16 lowercase hex digits.
pub(super) fn new_id() -> String {
    format!("{:016x}", rand::random::<u64>())
}

/// Whether `id` is shaped as [`new_id`] makes them. A teammate's ask id
/// reaches prompts, so nothing else is taken.
pub(super) fn is_ask_id(id: &str) -> bool {
    id.len() == 16 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

pub(super) fn too_long() -> String {
    format!("An ask holds at most {MAX_TEXT} characters")
}

#[cfg(test)]
#[path = "../tests/team_asks.rs"]
mod tests;
