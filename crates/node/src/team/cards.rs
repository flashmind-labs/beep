//! A session's card on its team's board, made from what this mesh lists of
//! it and nothing of its transcript, and the teammates' cards as their
//! gateways send them.

use crate::agents::mcp::children::ChildView;
use beep_proto::team::{Card, ForgeLogin, Lane};
use chrono::{DateTime, Local, NaiveTime};
use iroh::EndpointId;
use std::{collections::BTreeMap, time::SystemTime};

/// The most cards kept of one teammate.
pub(crate) const MAX_CARDS: usize = 200;
/// The longest card title a teammate's board shows, in characters.
pub(crate) const TITLE: usize = 140;

/// The titles of a repository's open issues, by number.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct IssueTitles(BTreeMap<u64, String>);

impl FromIterator<(u64, String)> for IssueTitles {
    fn from_iter<I: IntoIterator<Item = (u64, String)>>(titles: I) -> Self {
        Self(titles.into_iter().collect())
    }
}

/// What a card is made of besides its session.
pub(crate) struct Facts<'a> {
    /// The mesh's name for the team's project.
    pub(crate) project: &'a str,
    /// This mesh's login on the team.
    pub(crate) login: &'a ForgeLogin,
    pub(crate) linked: &'a IssueTitles,
    pub(crate) now: SystemTime,
}

/// `view`'s card, when it works in the team's project; `branch` is its
/// checkout's branch, when known.
pub(crate) fn card_of(view: &ChildView, facts: &Facts, branch: Option<&str>) -> Option<Card> {
    if view.project.as_deref() != Some(facts.project) {
        return None;
    }

    let lane = match view.state.as_str() {
        "waiting_on_user" => Lane::NeedsYou {},
        "working" => Lane::Working {},
        "idle" => Lane::ReadyToReview {},
        "ended" => Lane::DoneToday {},
        _ => return None,
    };
    let issue = branch.and_then(issue_of_branch).or(view.issue);
    let title = issue
        .and_then(|number| facts.linked.0.get(&number).cloned())
        .or_else(|| view.title.clone())
        .unwrap_or_else(|| view.harness.clone());

    Some(Card {
        id: view.id.clone(),
        title,
        lane,
        login: facts.login.clone(),
        machine: view.machine.clone(),
        issue,
        pr: None,
        updated_at: facts.now,
    })
}

/// The issue a branch named `<number>-...`, `issue-<number>-...` (as Work
/// on it names it) or `<who>/` either works on.
pub(crate) fn issue_of_branch(branch: &str) -> Option<u64> {
    let name = branch.rsplit('/').next()?;
    let name = name.strip_prefix("issue-").unwrap_or(name);
    let (number, _) = name.split_once('-').unwrap_or((name, ""));

    match number.bytes().all(|b| b.is_ascii_digit()) {
        true => number.parse().ok(),
        false => None,
    }
}

/// `fresh` as the board shows it `now`: a card that did not change since
/// `old` keeps the time it last changed, and one done before today goes.
pub(crate) fn settle(old: &[Card], fresh: Vec<Card>, now: SystemTime) -> Vec<Card> {
    let today = Into::<DateTime<Local>>::into(now).date_naive();

    fresh
        .into_iter()
        .map(|mut card| {
            let before = old.iter().find(|before| before.id == card.id);

            if let Some(before) = before
                && same(before, &card)
            {
                card.updated_at = before.updated_at;
            }

            card
        })
        .filter(|card| {
            card.lane != Lane::DoneToday {}
                || Into::<DateTime<Local>>::into(card.updated_at).date_naive() == today
        })
        .collect()
}

/// The start of the day of `now` here, in Unix seconds: sessions that ended
/// since are Done today, as the desktop's board has them.
pub(crate) fn midnight(now: SystemTime) -> u64 {
    let day = Into::<DateTime<Local>>::into(now).date_naive();
    let start = day.and_time(NaiveTime::MIN).and_local_timezone(Local).earliest();

    start.map_or(0, |start| u64::try_from(start.timestamp()).unwrap_or(0))
}

/// What changed from `old` to `new`: each card added or changed, then each
/// card gone (`true`).
pub(crate) fn changes(old: &[Card], new: &[Card]) -> Vec<(Card, bool)> {
    let changed = new
        .iter()
        .filter(|card| !old.contains(card))
        .map(|card| (card.clone(), false));
    let gone = old
        .iter()
        .filter(|card| !new.iter().any(|now| now.id == card.id))
        .map(|card| (card.clone(), true));

    changed.chain(gone).collect()
}

/// Whether two cards of one session say the same, but for when.
fn same(a: &Card, b: &Card) -> bool {
    Card {
        updated_at: b.updated_at,
        ..a.clone()
    } == *b
}

/// The cards of a team's other members, by their mesh. A member's gateway
/// speaks only for that member's login.
#[derive(Debug, Default)]
pub(crate) struct Teammates(BTreeMap<EndpointId, BTreeMap<String, Card>>);

impl Teammates {
    /// Takes `mesh`'s whole board, for the member `login`.
    pub(crate) fn board(&mut self, mesh: EndpointId, login: &ForgeLogin, cards: Vec<Card>) {
        let theirs = cards
            .into_iter()
            .filter(|card| card.login == *login)
            .take(MAX_CARDS)
            .map(|card| (card.id.clone(), capped(card)))
            .collect();

        self.0.insert(mesh, theirs);
    }

    /// Takes one card of `mesh`'s board, changed or `gone`.
    pub(crate) fn change(&mut self, mesh: EndpointId, login: &ForgeLogin, card: Card, gone: bool) {
        if card.login != *login {
            return;
        }

        let theirs = self.0.entry(mesh).or_default();

        if gone {
            theirs.remove(&card.id);
        } else if theirs.len() < MAX_CARDS || theirs.contains_key(&card.id) {
            theirs.insert(card.id.clone(), capped(card));
        }
    }

    /// Forgets `mesh`'s board: its gateway is out of reach or it left.
    pub(crate) fn gone(&mut self, mesh: EndpointId) {
        self.0.remove(&mesh);
    }

    /// The meshes of the teammates whose board is followed, cards or none.
    pub(crate) fn meshes(&self) -> impl Iterator<Item = EndpointId> + '_ {
        self.0.keys().copied()
    }

    /// The mesh of the teammate whose card `id` is.
    pub(crate) fn mesh_of(&self, id: &str) -> Option<EndpointId> {
        let mut theirs = self.0.iter();
        theirs.find_map(|(mesh, cards)| cards.contains_key(id).then_some(*mesh))
    }

    /// Every teammate's cards.
    pub(crate) fn cards(&self) -> Vec<Card> {
        self.0
            .values()
            .flat_map(|cards| cards.values().cloned())
            .collect()
    }
}

/// `card` with its title cut to `TITLE`.
fn capped(mut card: Card) -> Card {
    if let Some((at, _)) = card.title.char_indices().nth(TITLE) {
        card.title.truncate(at);
    }

    card
}

#[cfg(test)]
#[path = "../tests/team_cards.rs"]
mod tests;
