//! Asks and answers from teammates, as this mesh's gateway takes them: an
//! ask goes to the session on the card it names, or becomes a Needs you card
//! for this mesh's own apps when it is for the person; an answer goes into
//! the session that asked. Their words are a teammate's, so they reach a
//! session escaped, as a message from that teammate's agent.

use super::{
    asks::{
        ASK_CARD, Delivery, MAX_CARD, MAX_CARDS, MAX_ID, MAX_TEXT, MAX_WAITING, NOT_WAITING, Taken,
        not_served, too_long,
    },
    link::{TeamLink, Teams},
};
use crate::agents::tags;
use beep_proto::team::{Card, Lane, Target, TeamMsg, TeamRepo};
use iroh::EndpointId;
use std::time::SystemTime;

impl TeamLink {
    /// Takes an ask from `peer`, a device of a teammate: hands it to the
    /// session on the card it names, or keeps it as a Needs you card for
    /// this mesh's person. The refusal when it is not taken.
    pub(super) async fn asked(&self, peer: EndpointId, ask: &TeamMsg) -> Option<TeamMsg> {
        let TeamMsg::Ask { id, from, text, .. } = ask else {
            return None;
        };
        let taken = self.with(|teams| take(teams, peer, ask, SystemTime::now()));
        let (repo, card) = match taken {
            Ok(Some(taken)) => taken,
            Ok(None) => return None,
            Err(reason) => return Some(refused(reason)),
        };
        let words = format!(
            "{text}\n\n[An ask from {}'s agent on the team of {repo}. Answer it once with \
             team_ask, reply_to {id}.]",
            from.login
        );
        let words = tags::from_teammate(&from.card, &format!("{}'s agent", from.login), &words);
        let delivered = self.deliver(&card, Delivery::Ask, words).await;

        delivered.err().map(|reason| {
            self.with(|teams| teams.asks.taken.remove(id));
            refused(reason)
        })
    }

    /// Takes `peer`'s answer to an ask this mesh sent, into the asking session.
    pub(super) async fn answered(&self, peer: EndpointId, id: &str, text: &str) -> Option<TeamMsg> {
        let sent = self.with(|teams| {
            teams.asks.expire(SystemTime::now());
            let sent = teams.asks.sent.get(id).ok_or(NOT_WAITING)?;
            let from = teams.rosters.get(&sent.repo).and_then(|record| {
                let mut members = record.members.iter();
                members.find(|member| member.devices.iter().any(|device| device.id == peer))
            });

            match from {
                Some(member) if member.mesh == sent.mesh && text.chars().count() <= MAX_TEXT => {
                    let login = member.login.clone();
                    Ok((teams.asks.sent.remove(id).ok_or(NOT_WAITING)?, login))
                }
                Some(_) => Err("Only the one asked answers an ask, within 4000 characters"),
                None => Err(NOT_WAITING),
            }
        });
        let (sent, login) = match sent {
            Ok(sent) => sent,
            Err(reason) => return Some(refused(reason.to_owned())),
        };
        let words = format!("{text}\n\n[{login}'s answer to your team_ask {id}.]");
        let words = tags::from_teammate(&sent.asked, &format!("{login}'s agent"), &words);

        let delivered = self.deliver(&sent.asker, Delivery::Reply, words).await;
        delivered.err().map(refused)
    }

    async fn deliver(&self, card: &str, delivery: Delivery, text: String) -> Result<(), String> {
        match self.sessions() {
            Some(sessions) => sessions.deliver(card, delivery, text).await,
            None => Err("This device runs no sessions".to_owned()),
        }
    }
}

/// Checks `ask` from `peer` and keeps it: its repository and the card of the
/// session to hand it to, `None` once it is a Needs you card, or the refusal.
fn take(
    teams: &mut Teams,
    peer: EndpointId,
    ask: &TeamMsg,
    now: SystemTime,
) -> Result<Option<(TeamRepo, String)>, String> {
    let TeamMsg::Ask {
        id,
        repo,
        from,
        to,
        text,
        ..
    } = ask
    else {
        return Ok(None);
    };
    let own = teams.home.mesh;
    let record = teams
        .rosters
        .get(repo)
        .filter(|_| teams.boards.serves(repo))
        .ok_or_else(|| not_served(repo))?;
    let mine = record
        .members
        .iter()
        .find(|member| Some(member.mesh) == own)
        .ok_or_else(|| not_served(repo))?;
    let sender = record
        .members
        .iter()
        .find(|member| member.devices.iter().any(|device| device.id == peer))
        .ok_or_else(|| not_served(repo))?;
    // The asking session is one the sender's own board shows.
    let asker = teams
        .teammates
        .get(repo)
        .and_then(|cards| cards.mesh_of(&from.card));

    if from.login != sender.login {
        return Err("An ask comes in its sender's own name".to_owned());
    }
    if from.card.len() > MAX_CARD || asker != Some(sender.mesh) {
        return Err("An ask comes from a session on its sender's board".to_owned());
    }
    if !record.asks {
        return Err(format!(
            "{} does not take messages from teammates' agents",
            mine.login
        ));
    }
    if text.chars().count() > MAX_TEXT {
        return Err(too_long());
    }

    teams.asks.expire(now);

    if id.is_empty() || id.len() > MAX_ID || teams.asks.taken.contains_key(id) {
        return Err("An ask needs a new short id".to_owned());
    }
    if teams.asks.taken.len() >= MAX_WAITING {
        return Err("Too many asks wait here for answers".to_owned());
    }

    let (login, mesh) = (mine.login.clone(), sender.mesh);
    let card = match to {
        Target::Person { login: person } if *person == login => format!("{ASK_CARD}{id}"),
        Target::Person { login: person } => return Err(format!("{person} is not on this mesh")),
        Target::Agent { card } => {
            let cards = teams.boards.cards(repo).unwrap_or_default();
            let on_board = cards.iter().any(|on| on.id == *card);

            if !on_board {
                return Err(format!("No session {card} on {login}'s board"));
            }
            card.clone()
        }
    };
    let taken = Taken {
        repo: repo.clone(),
        card: card.clone(),
        mesh,
        at: now,
    };

    if !card.starts_with(ASK_CARD) {
        teams.asks.taken.insert(id.clone(), taken);
        return Ok(Some((repo.clone(), card)));
    }

    let cards = teams.asks.cards.entry(repo.clone()).or_default();

    if cards.len() >= MAX_CARDS {
        return Err(format!("Too many asks wait for {login}"));
    }

    // The whole ask: the card is this mesh's own, and answers are made from it.
    cards.push(Card {
        id: card,
        title: text.clone(),
        lane: Lane::NeedsYou {},
        login,
        machine: String::new(),
        issue: None,
        pr: None,
        updated_at: now,
    });
    teams.asks.taken.insert(id.clone(), taken);

    Ok(None)
}

fn refused(reason: String) -> TeamMsg {
    TeamMsg::Refused { reason }
}
