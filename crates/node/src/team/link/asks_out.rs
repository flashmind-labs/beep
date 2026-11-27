//! The asks a device sends over the team link.

use super::{Seen, TeamError, TeamLink, Teams, finish};
use crate::team::asks::{
    ASK_CARD, Asks, MAX_TEXT, MAX_WAITING, NOT_WAITING, Outgoing, Sent, Taken, new_id, not_served,
    too_long,
};
use beep_proto::team::{Member, Target, TeamMsg, TeamRepo};
use iroh::EndpointAddr;
use std::time::{Duration, SystemTime};
use tokio::time::timeout;

/// How long reaching a teammate may take over all their devices, and one
/// device to say whether it took a message.
const REACH: Duration = Duration::from_secs(60);
const ANSWER: Duration = Duration::from_secs(40);

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
    pub(in crate::team) async fn reach(
        &self,
        member: &Member,
        msg: &TeamMsg,
    ) -> Result<(), String> {
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
    pub(in crate::team) async fn offer(
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
