//! Asks between the agents of a team. A session's `team_ask` goes through
//! its mesh's gateway to the teammate's gateway, which hands it to the
//! session on the card it names, or keeps it as a Needs you card for this
//! mesh's apps when it is for the person (`incoming`). The session asked
//! answers once, with `team_ask` and the ask's id, and the answer goes back
//! the same way into the asking session.

use super::link::{Seen, TeamError, TeamLink, Teams, finish};
use beep_proto::{
    SessionId,
    team::{AgentRef, Card, Member, Target, TeamMsg, TeamRepo},
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

impl TeamLink {
    /// Sends `ask`, from a session of this mesh, as the mesh's gateway: a
    /// new ask to a teammate, or the answer to one this mesh took. The ask's id.
    pub(crate) async fn send_ask(&self, ask: Outgoing) -> Result<String, String> {
        if ask.text.chars().count() > MAX_TEXT {
            return Err(too_long());
        }

        let now = SystemTime::now();
        let card = match &ask.to {
            Some(Target::Agent { card }) => Some(card.as_str()),
            _ => None,
        };
        let seen = self.seen(&ask.repo, card, None);
        let (member, msg, id, answering) =
            self.both(|teams, asks| outgoing(teams, asks, &seen, ask, now))?;

        if let Err(e) = self.reach(&member, &msg).await {
            // An answer not sent still waits; an ask not sent waits for nothing.
            self.asks(|asks| match answering {
                Some(taken) => drop(asks.taken.insert(id, taken)),
                None => drop(asks.sent.remove(&id)),
            });
            return Err(e);
        }

        // An ask for the person is answered: its Needs you card goes.
        if let Some(taken) = answering {
            let card = format!("{ASK_CARD}{id}");
            self.asks(|asks| {
                if let Some(cards) = asks.cards.get_mut(&taken.repo) {
                    cards.retain(|kept| kept.id != card);
                }
            });
            self.cards_changed(&taken.repo);
        }

        Ok(id)
    }

    /// Drops the dismissed Needs you card `id` of `repo`: an ask's, and the
    /// ask with it, or that of an automatic session that ended without a
    /// pull request (`pickup`).
    pub(crate) fn dismiss_card(&self, repo: &TeamRepo, id: &str) {
        self.asks(|asks| {
            if let Some(cards) = asks.cards.get_mut(repo) {
                cards.retain(|card| card.id != id);
            }
            if let Some(ask) = id.strip_prefix(ASK_CARD) {
                asks.taken.remove(ask);
            }
        });
        self.cards_changed(repo);
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
                match self.offer(std::slice::from_ref(device), msg).await {
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

    /// Sends `msg` to the first of `devices` that answers: `None` when it
    /// took it, else its refusal.
    pub(super) async fn offer(
        &self,
        devices: &[EndpointAddr],
        msg: &TeamMsg,
    ) -> Result<Option<String>, TeamError> {
        let (mut tx, mut rx) = self.dial(devices).await?;
        tx.send(msg).await?;
        // The device answers once it handed an ask on or checked a roster's
        // newcomers on the forge, which may take a while.
        let (_, answer) = tokio::join!(finish(tx), timeout(ANSWER, rx.recv::<TeamMsg>()));
        let answer = answer.map_err(|_| TeamError::Refused("no answer".to_owned()))??;

        Ok(match answer {
            Some(TeamMsg::Refused { reason }) => Some(reason),
            _ => None,
        })
    }
}

/// `ask` made a message to a teammate: the member it goes to, the message,
/// its id, and the ask it answers, taken off what waits.
fn outgoing(
    teams: &Teams,
    asks: &mut Asks,
    seen: &Seen,
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

    if !seen.serves {
        return Err("This machine no longer acts for the team; try again in a minute".to_owned());
    }

    asks.expire(now);
    let own = seen.own;
    let record = teams.rosters.get(&repo).ok_or("This project has no team")?;
    let mut others = record
        .members
        .iter()
        .filter(|member| Some(member.mesh) != own);

    if let Some(id) = reply_to {
        // An ask for the person waits for whichever session they answer from.
        let waits = |taken: &Taken| {
            taken.repo == repo && (taken.card == from.card || taken.card.starts_with(ASK_CARD))
        };
        let no_ask = || format!("No ask {id} waits for an answer from this session");
        let taken = asks
            .taken
            .get(&id)
            .filter(|taken| waits(taken))
            .ok_or_else(no_ask)?;
        let member = others
            .find(|member| member.mesh == taken.mesh)
            .ok_or_else(no_ask)?
            .clone();
        let taken = asks.taken.remove(&id);
        let reply = TeamMsg::Reply {
            id: id.clone(),
            text,
        };
        return Ok((member, reply, id, taken));
    }

    let to = to.ok_or("Give `to`: a card id or a teammate's login.")?;
    let (asked, member) = match &to {
        Target::Person { login } => (
            login.to_string(),
            others.find(|member| member.login == *login),
        ),
        Target::Agent { card } => (
            card.clone(),
            others.find(|member| Some(member.mesh) == seen.teammate_mesh),
        ),
    };
    let member = member
        .ok_or_else(|| {
            format!("No teammate or teammate's card {asked}. The team tool lists them.")
        })?
        .clone();

    if asks.sent.len() >= MAX_WAITING {
        return Err("Too many asks wait for answers; try again later".to_owned());
    }

    let id = new_id();
    let sent = Sent {
        repo: repo.clone(),
        asker: from.card.clone(),
        mesh: member.mesh,
        asked,
        at: now,
    };
    asks.sent.insert(id.clone(), sent);
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

/// A new ask's id: 16 lowercase hex digits.
fn new_id() -> String {
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
