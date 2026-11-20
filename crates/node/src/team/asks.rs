//! Asks between the agents of a team. A session's `team_ask` goes through
//! its mesh's gateway to the teammate's gateway, which hands it to the
//! session on the card it names, or puts it on its board as a Needs you
//! card when it is for the person. The session asked answers once, with
//! `team_ask` and the ask's id, and the answer goes back the same way into
//! the asking session.

use super::link::{TeamError, TeamLink, Teams, finish};
use crate::agents::tags;
use beep_proto::{
    SessionId,
    team::{AgentRef, Card, Lane, Member, Target, TeamMsg, TeamRepo},
};
use futures::future::BoxFuture;
use iroh::{EndpointAddr, EndpointId};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    time::{Duration, Instant, SystemTime},
};
use tokio::time::timeout;

/// The longest ask or answer, in characters.
pub(crate) const MAX_TEXT: usize = 4000;
/// What a device that holds no ask by an answer's id says: the sender
/// tries the teammate's next device.
pub(super) const NOT_WAITING: &str = "No ask waits for this answer here";
/// How many asks a session sends in an hour.
const PER_HOUR: usize = 10;
const HOUR: Duration = Duration::from_secs(60 * 60);
/// How long an ask waits for its answer, and its Needs you card stays.
const KEPT_FOR: Duration = Duration::from_secs(24 * 60 * 60);
/// The most asks waiting each way, and Needs you cards on one board.
const MAX_WAITING: usize = 256;
const MAX_CARDS: usize = 32;
/// The longest Needs you card title, in characters, and ask id.
const TITLE: usize = 140;
const MAX_ID: usize = 64;
/// How long reaching a teammate may take over all their devices, and one
/// device to say whether it took a message.
const REACH: Duration = Duration::from_secs(60);
const ANSWER: Duration = Duration::from_secs(40);

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

/// The sessions of this mesh, which asks and answers are handed to.
pub(crate) trait Sessions: Send + Sync + 'static {
    /// Puts `text` into the session `card` (`<session>@<machine>`) names.
    fn deliver<'a>(
        &'a self,
        card: &'a str,
        delivery: Delivery,
        text: String,
    ) -> BoxFuture<'a, Result<(), String>>;
}

/// `Tool::TeamAsk` from a session's node to its mesh's gateway: the ask as
/// that node made it, or the answer to one when `reply_to` is set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Outgoing {
    pub(crate) repo: TeamRepo,
    pub(crate) from: AgentRef,
    pub(crate) to: Target,
    pub(crate) text: String,
    pub(crate) reply_to: Option<String>,
}

/// The asks this device keeps as its mesh's gateway.
#[derive(Debug, Default)]
pub(crate) struct Asks {
    /// What this device's sessions sent, so one does not flood the team.
    pub(crate) limit: AskLimit,
    /// Sent to teammates, by id, until answered.
    sent: BTreeMap<String, Sent>,
    /// Taken from teammates, by id, until answered once.
    taken: BTreeMap<String, Taken>,
    /// The asks for this mesh's person, as Needs you cards.
    cards: BTreeMap<TeamRepo, Vec<Card>>,
}

#[derive(Debug, Clone)]
struct Sent {
    repo: TeamRepo,
    /// The asking session's card.
    asker: String,
    /// The mesh of the member asked, the only one who answers.
    mesh: EndpointId,
    /// The card or login asked.
    asked: String,
    at: SystemTime,
}

#[derive(Debug, Clone)]
struct Taken {
    repo: TeamRepo,
    /// The card asked: its session answers.
    card: String,
    /// The mesh of the member who asked.
    mesh: EndpointId,
    at: SystemTime,
}

impl Asks {
    /// The Needs you cards of `repo`'s asks.
    pub(crate) fn cards(&mut self, repo: &TeamRepo, now: SystemTime) -> Vec<Card> {
        self.expire(now);
        self.cards.get(repo).cloned().unwrap_or_default()
    }

    fn expire(&mut self, now: SystemTime) {
        let fresh = |at: SystemTime| now.duration_since(at).map_or(true, |age| age < KEPT_FOR);
        self.sent.retain(|_, sent| fresh(sent.at));
        self.taken.retain(|_, taken| fresh(taken.at));
        self.cards.retain(|_, cards| {
            cards.retain(|card| fresh(card.updated_at));
            !cards.is_empty()
        });
    }
}

impl TeamLink {
    /// Sends `ask`, from a session of this mesh, as the mesh's gateway: a
    /// new ask to a teammate, or the answer to one this mesh took. The ask's id.
    pub(crate) async fn send_ask(&self, ask: Outgoing) -> Result<String, String> {
        if ask.text.chars().count() > MAX_TEXT {
            return Err(too_long());
        }

        let now = SystemTime::now();
        let (member, msg, id, answering) = self.with(|teams| outgoing(teams, ask, now))?;

        if let Err(e) = self.reach(&member, &msg).await {
            // An answer not sent still waits; an ask not sent waits for nothing.
            self.with(|teams| match answering {
                Some(taken) => drop(teams.asks.taken.insert(id, taken)),
                None => drop(teams.asks.sent.remove(&id)),
            });
            return Err(e);
        }

        Ok(id)
    }

    /// Takes the dismissed ask card `id` off `repo`'s board.
    #[cfg_attr(not(test), expect(dead_code, reason = "the Team page dismisses asks"))]
    pub(crate) fn dismiss_ask(&self, repo: &TeamRepo, id: &str) {
        self.with(|teams| {
            if let Some(cards) = teams.asks.cards.get_mut(repo) {
                cards.retain(|card| card.id != id);
            }
            if let Some(mut cards) = teams.boards.cards(repo) {
                cards.retain(|card| card.id != id);
                teams.boards.set(repo, cards);
            }
        });
    }

    /// Sends `msg` to the first of `member`'s devices that takes it. A device
    /// that is not their gateway, or holds no ask an answer is for, passes.
    pub(super) async fn reach(&self, member: &Member, msg: &TeamMsg) -> Result<(), String> {
        let passes = |reason: &str| match msg {
            TeamMsg::Ask { repo, .. } => reason == not_served(repo),
            _ => reason == NOT_WAITING,
        };
        let tries = async {
            let mut said = TeamError::Unreachable.to_string();

            for device in &member.devices {
                match self.offer(device, msg).await {
                    Ok(None) => return Ok(()),
                    Ok(Some(reason)) if passes(&reason) => said = reason,
                    Ok(Some(reason)) => return Err(reason),
                    Err(e) => log::debug!("an ask did not reach {}: {e}", device.id.fmt_short()),
                }
            }

            Err(said)
        };

        timeout(REACH, tries)
            .await
            .unwrap_or_else(|_| Err(format!("{} did not answer in time", member.login)))
    }

    /// Sends `msg` to `device`: `None` when it took it, else its refusal.
    async fn offer(
        &self,
        device: &EndpointAddr,
        msg: &TeamMsg,
    ) -> Result<Option<String>, TeamError> {
        let (mut tx, mut rx) = self.dial(std::slice::from_ref(device)).await?;
        tx.send(msg).await?;
        // The device answers once it handed the ask on, which may take a while.
        let (_, answer) = tokio::join!(finish(tx), timeout(ANSWER, rx.recv::<TeamMsg>()));
        let answer = answer.map_err(|_| TeamError::Refused("no answer".to_owned()))??;

        Ok(match answer {
            Some(TeamMsg::Refused { reason }) => Some(reason),
            _ => None,
        })
    }

    /// Takes an ask from `peer`, a device of a teammate: hands it to the
    /// session on the card it names, or makes it a Needs you card for this
    /// mesh's person. The refusal when it is not taken.
    pub(super) async fn asked(&self, peer: EndpointId, ask: &TeamMsg) -> Option<TeamMsg> {
        let TeamMsg::Ask { id, from, text, .. } = ask else {
            return None;
        };
        let taken = self.with(|teams| take(teams, peer, ask, SystemTime::now()));
        let (repo, card) = match taken {
            Ok(Some(taken)) => taken,
            Ok(None) => return None,
            Err(reason) => return Some(TeamMsg::Refused { reason }),
        };
        let words = format!(
            "{text}\n\n[An ask from {}'s agent on the team of {repo}. Answer it once with \
             team_ask: to {}, reply_to {id}.]",
            from.login, from.card
        );
        let words = tags::from_session(&from.card, &format!("{}'s agent", from.login), &words);
        let delivered = self.deliver(&card, Delivery::Ask, words).await;

        delivered.err().map(|reason| {
            self.with(|teams| teams.asks.taken.remove(id));
            TeamMsg::Refused { reason }
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
            Err(reason) => return Some(refused(reason)),
        };
        let words = format!("{text}\n\n[{login}'s answer to your team_ask {id}.]");
        let words = tags::from_session(&sent.asked, &format!("{login}'s agent"), &words);

        let delivered = self.deliver(&sent.asker, Delivery::Reply, words).await;
        delivered.err().map(|reason| TeamMsg::Refused { reason })
    }

    async fn deliver(&self, card: &str, delivery: Delivery, text: String) -> Result<(), String> {
        match self.sessions() {
            Some(sessions) => sessions.deliver(card, delivery, text).await,
            None => Err("This device runs no sessions".to_owned()),
        }
    }
}

/// `ask` made a message to a teammate: the member it goes to, the message,
/// its id, and the ask it answers, taken off what waits.
fn outgoing(
    teams: &mut Teams,
    ask: Outgoing,
    now: SystemTime,
) -> Result<(Member, TeamMsg, String, Option<Taken>), String> {
    let Outgoing {
        repo,
        from,
        to,
        text,
        reply_to,
    } = ask;

    if !teams.boards.serves(&repo) {
        return Err("This machine no longer acts for the team; try again in a minute".to_owned());
    }

    teams.asks.expire(now);
    let own = teams.home.mesh;
    let record = teams.rosters.get(&repo).ok_or("This project has no team")?;
    let mut others = record
        .members
        .iter()
        .filter(|member| Some(member.mesh) != own);

    if let Some(id) = reply_to {
        let waits = |taken: &Taken| taken.repo == repo && taken.card == from.card;
        let no_ask = || format!("No ask {id} waits for an answer from this session");
        let taken = teams
            .asks
            .taken
            .get(&id)
            .filter(|taken| waits(taken))
            .ok_or_else(no_ask)?;
        let member = others
            .find(|member| member.mesh == taken.mesh)
            .ok_or_else(no_ask)?
            .clone();
        let taken = teams.asks.taken.remove(&id);
        let reply = TeamMsg::Reply {
            id: id.clone(),
            text,
        };
        return Ok((member, reply, id, taken));
    }

    let (asked, member) = match &to {
        Target::Person { login } => (
            login.to_string(),
            others.find(|member| member.login == *login),
        ),
        Target::Agent { card } => {
            let mesh = teams
                .teammates
                .get(&repo)
                .and_then(|cards| cards.mesh_of(card));
            (
                card.clone(),
                others.find(|member| Some(member.mesh) == mesh),
            )
        }
    };
    let member = member
        .ok_or_else(|| {
            format!("No teammate or teammate's card {asked}. The team tool lists them.")
        })?
        .clone();

    if teams.asks.sent.len() >= MAX_WAITING {
        return Err("Too many asks wait for answers; try again later".to_owned());
    }

    let id = format!("{:016x}", rand::random::<u64>());
    let sent = Sent {
        repo: repo.clone(),
        asker: from.card.clone(),
        mesh: member.mesh,
        asked,
        at: now,
    };
    teams.asks.sent.insert(id.clone(), sent);
    let msg = TeamMsg::Ask {
        id: id.clone(),
        repo,
        from,
        to,
        text,
        reply_to: None,
    };

    Ok((member, msg, id, None))
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

    if from.login != sender.login {
        return Err("An ask comes in its sender's own name".to_owned());
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
        Target::Person { login: person } if *person == login => format!("ask:{id}"),
        Target::Person { login: person } => return Err(format!("{person} is not on this mesh")),
        Target::Agent { card } => {
            // A session waiting on its user would take the words as the answer.
            let cards = teams.boards.cards(repo).unwrap_or_default();
            let on_board = cards
                .iter()
                .any(|on| on.id == *card && on.lane != Lane::NeedsYou {});

            if !on_board || card.starts_with("ask:") {
                return Err(format!(
                    "No session {card} on {login}'s board takes asks now"
                ));
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
    teams.asks.taken.insert(id.clone(), taken);

    if !card.starts_with("ask:") {
        return Ok(Some((repo.clone(), card)));
    }

    let cards = teams.asks.cards.entry(repo.clone()).or_default();

    if cards.len() >= MAX_CARDS {
        teams.asks.taken.remove(id);
        return Err(format!("Too many asks wait for {login}"));
    }

    let ask_card = Card {
        id: card,
        title: text.chars().take(TITLE).collect(),
        lane: Lane::NeedsYou {},
        login,
        machine: String::new(),
        issue: None,
        pr: None,
        updated_at: now,
    };
    cards.push(ask_card.clone());
    let mut board = teams.boards.cards(repo).unwrap_or_default();
    board.push(ask_card);
    teams.boards.set(repo, board);

    Ok(None)
}

fn too_long() -> String {
    format!("An ask holds at most {MAX_TEXT} characters")
}

fn refused(reason: &str) -> TeamMsg {
    TeamMsg::Refused {
        reason: reason.to_owned(),
    }
}

#[cfg(test)]
#[path = "../tests/team_asks.rs"]
mod tests;
