//! A team's board over the link: the boards this device serves to the
//! teammates who subscribe, and the follow of a teammate's board.

use super::cards::changes;
use beep_proto::team::{Card, TeamRepo};
use std::collections::{BTreeMap, BTreeSet};
use tokio::sync::broadcast;

/// Changes kept for a slow subscriber before it gets the whole board again.
pub(super) const CHANGES: usize = 64;

/// A board's cards, and what changes on it from then on.
type Watched = (Vec<Card>, broadcast::Receiver<(Card, bool)>);

/// The boards this device serves, for the teammates subscribed to them.
#[derive(Debug, Default)]
pub(crate) struct Boards(BTreeMap<TeamRepo, Board>);

#[derive(Debug)]
struct Board {
    cards: Vec<Card>,
    changes: broadcast::Sender<(Card, bool)>,
}

impl Boards {
    /// Serves `repo`'s board with `cards`, telling its subscribers what changed.
    pub(crate) fn set(&mut self, repo: &TeamRepo, cards: Vec<Card>) {
        let board = self.0.entry(repo.clone()).or_insert_with(|| Board {
            cards: Vec::new(),
            changes: broadcast::channel(CHANGES).0,
        });

        for change in changes(&board.cards, &cards) {
            let _ = board.changes.send(change);
        }

        board.cards = cards;
    }

    /// Serves only `held`'s boards: the others' subscribers are let go.
    pub(super) fn keep(&mut self, held: &BTreeSet<TeamRepo>) {
        self.0.retain(|repo, _| held.contains(repo));
    }

    pub(crate) fn serves(&self, repo: &TeamRepo) -> bool {
        self.0.contains_key(repo)
    }

    pub(crate) fn cards(&self, repo: &TeamRepo) -> Option<Vec<Card>> {
        Some(self.0.get(repo)?.cards.clone())
    }

    /// `repo`'s board now, and what changes on it from now on.
    pub(in crate::team) fn watch(&self, repo: &TeamRepo) -> Option<Watched> {
        let board = self.0.get(repo)?;

        Some((board.cards.clone(), board.changes.subscribe()))
    }
}

#[cfg(test)]
#[path = "../tests/team_board.rs"]
pub(super) mod tests;
