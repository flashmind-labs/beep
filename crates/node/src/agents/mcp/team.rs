//! The team tools, for sessions in a team project: `team` lists the team,
//! `team_ask` asks a teammate's agent or a teammate, or answers an ask. The
//! session's node makes the call and sends it to the machine of its mesh
//! that acts for the team (`team::gateway`), which answers it; that machine
//! hands asks and answers to the sessions they are for (`Tool::TeamDeliver`).

use super::{
    children::{full_id, machine_name},
    machines::{find, over_the_mesh},
    reply::{fail, json_of, ok, parse},
};
use crate::{
    agents::Agents,
    link::Offers,
    team::{
        asks::{Delivery, MAX_TEXT, Outgoing, Sessions},
        cards::Teammates,
        link::TeamLink,
    },
};
use beep_mesh::Mesh;
use beep_proto::{
    SessionId,
    node::tools::{Approved, Reply, Request, Tool},
    team::{AgentRef, Card, Target, TeamRepo},
};
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::{Duration, Instant};
use tokio::time::timeout;

/// How long handing an ask to a session on another machine, or reading the
/// team's issues, may take.
const LIMIT: Duration = Duration::from_secs(30);

/// `team_ask` as the agent calls it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AskArgs {
    to: Option<String>,
    text: String,
    reply_to: Option<String>,
}

/// `Tool::Team` from a session's node to the gateway.
#[derive(Serialize, Deserialize)]
struct TeamArgs {
    repo: TeamRepo,
}

/// `Tool::TeamDeliver`: a text for the session on the card `card`, which
/// runs on the machine that answers.
#[derive(Serialize, Deserialize)]
struct Handed {
    card: String,
    delivery: Delivery,
    text: String,
}

/// The team link and the team `session`'s project is on, when it is on one.
pub(super) fn team_of(offers: &Offers, session: &SessionId) -> Option<(TeamLink, TeamRepo)> {
    let link = offers.reach.peers()?.team();
    let folder = offers.agents.folder_of(session)?;
    let repo = link.with(|teams| {
        let repo = teams.home.team_of(&folder)?;
        teams.rosters.get(&repo).map(|_| repo)
    })?;

    Some((link, repo))
}

/// `team` or `team_ask` from a session on this node, answered by the
/// machine of the mesh that acts for the team.
pub(super) async fn call(offers: &Offers, request: &Request) -> Reply {
    let (Some(mesh), Some((link, repo))) = (offers.mesh.get(), team_of(offers, &request.session))
    else {
        return fail("This session's project has no team.");
    };
    let (args, asking) = match request.tool {
        Tool::TeamAsk => match outgoing(&offers.agents, &mesh, &link, request, repo.clone()) {
            Ok(ask) => (serde_json::to_string(&ask), ask.reply_to.is_none()),
            Err(e) => return fail(e),
        },
        _ => (
            serde_json::to_string(&TeamArgs { repo: repo.clone() }),
            false,
        ),
    };
    let args = match args {
        Ok(args) => args,
        Err(e) => return fail(e.to_string()),
    };
    let request = Request {
        session: request.session.clone(),
        tool: request.tool,
        args,
        approved: Approved::No,
    };

    // Only new asks count against the limit, and only once they are sent.
    let counted = match asking {
        true => link.with(|teams| teams.asks.limit.take(&request.session, Instant::now())),
        false => Ok(()),
    };
    if let Err(e) = counted {
        return fail(e);
    }

    let reply = match link.with(|teams| teams.gateways.get(&repo).copied()) {
        Some(gateway) if gateway == mesh.me() => answer(&link, &offers.agents, &request).await,
        Some(gateway) => over_the_mesh(&mesh, gateway, &request)
            .await
            .unwrap_or_else(|e| {
                fail(format!(
                    "The machine that acts for the team did not answer: {e:#}"
                ))
            }),
        None => fail("No machine of this mesh acts for the team yet; try again in a minute."),
    };

    if asking && matches!(reply, Reply::Done { failed: true, .. }) {
        link.with(|teams| teams.asks.limit.give_back(&request.session));
    }

    reply
}

/// The ask `request` makes, from its session's card: to a teammate when it
/// names one by login, else to the card it names.
fn outgoing(
    agents: &Agents,
    mesh: &Mesh,
    link: &TeamLink,
    request: &Request,
    repo: TeamRepo,
) -> Result<Outgoing, String> {
    let AskArgs { to, text, reply_to } = parse(&request.args)?;
    let to = to.as_deref().map(str::trim).filter(|to| !to.is_empty());

    if text.trim().is_empty() {
        return Err("Give `text`.".to_owned());
    }
    if to.is_none() && reply_to.is_none() {
        return Err("Give `to`: a card id or a teammate's login.".to_owned());
    }
    if text.chars().count() > MAX_TEXT {
        return Err(format!(
            "team_ask takes at most {MAX_TEXT} characters of text."
        ));
    }

    let session = agents
        .get(&request.session)
        .ok_or("This session no longer runs.")?;
    let card = full_id(
        Agents::name_of(&session).as_ref(),
        &machine_name(Some(mesh)),
    );
    let (login, to) = link.with(|teams| {
        let record = teams
            .rosters
            .get(&repo)
            .ok_or("This project has no team.")?;
        let own = teams.home.mesh;
        let mut members = record.members.iter();
        let login = members
            .find(|member| Some(member.mesh) == own)
            .ok_or("This mesh is not on the team yet.")?
            .login
            .clone();
        let named = |to: &str| {
            let mut others = record
                .members
                .iter()
                .filter(|member| Some(member.mesh) != own);
            others.find(|member| member.login.to_string() == to)
        };
        // An answer goes to whoever asked: its `to` is not needed.
        let to = to.filter(|_| reply_to.is_none()).map(|to| match named(to) {
            Some(member) => Target::Person {
                login: member.login.clone(),
            },
            None => Target::Agent {
                card: to.to_owned(),
            },
        });
        Ok::<_, String>((login, to))
    })?;

    Ok(Outgoing {
        repo,
        from: AgentRef { login, card },
        to,
        text,
        reply_to,
    })
}

/// The answer of the machine that acts for the team to `team` and
/// `team_ask`, and of a session's machine to `TeamDeliver`.
pub(crate) async fn answer(link: &TeamLink, agents: &Agents, request: &Request) -> Reply {
    match request.tool {
        Tool::Team => match parse::<TeamArgs>(&request.args) {
            Ok(TeamArgs { repo }) => team(link, &repo).await,
            Err(e) => fail(e),
        },
        Tool::TeamAsk => {
            let ask = match parse::<Outgoing>(&request.args) {
                Ok(ask) => ask,
                Err(e) => return fail(e),
            };
            let answering = ask.reply_to.is_some();

            match link.send_ask(ask).await {
                Ok(_) if answering => ok("Sent your answer."),
                Ok(id) => ok(format!(
                    "Sent as ask {id}. The answer arrives in this session as a message."
                )),
                Err(e) => fail(e),
            }
        }
        Tool::TeamDeliver => match parse::<Handed>(&request.args) {
            Ok(Handed {
                card,
                delivery,
                text,
            }) => match here(agents, &card, delivery, text).await {
                Ok(()) => ok("Delivered."),
                Err(e) => fail(e),
            },
            Err(e) => fail(e),
        },
        other => fail(format!("{other} is not a team tool.")),
    }
}

/// The team of `repo`: its members, this mesh's and the teammates' cards,
/// and its open issues with their assignees.
async fn team(link: &TeamLink, repo: &TeamRepo) -> Reply {
    let view = link.with(|teams| {
        let ours = teams.boards.cards(repo)?;
        let members: Vec<String> = teams
            .rosters
            .get(repo)?
            .members
            .iter()
            .map(|member| member.login.to_string())
            .collect();
        let theirs = teams.teammates.get(repo).map(Teammates::cards);
        let cards: Vec<Card> = ours.into_iter().chain(theirs.unwrap_or_default()).collect();
        Some((members, cards))
    });
    let Some((members, cards)) = view else {
        return fail("This machine no longer acts for the team; try again in a minute.");
    };
    let issues = match timeout(LIMIT, link.forge().open_issues(repo)).await {
        Ok(Ok(issues)) => issues
            .into_iter()
            .map(|issue| json!({ "number": issue.number, "title": issue.title, "assignees": issue.assignees }))
            .collect(),
        Ok(Err(e)) => json!(format!("Could not read the issues: {e}")),
        Err(_) => json!("Reading the issues took too long."),
    };
    let cards: Vec<_> = cards
        .into_iter()
        .map(|card| {
            json!({
                "id": card.id,
                "title": card.title,
                "lane": Into::<&'static str>::into(card.lane),
                "login": card.login.to_string(),
                "machine": card.machine,
                "issue": card.issue,
                "pr": card.pr,
            })
        })
        .collect();

    json_of(
        &json!({ "repo": repo.to_string(), "members": members, "cards": cards, "issues": issues }),
    )
}

/// The sessions of this node's mesh, which the team link hands asks to.
pub(crate) struct TeamSessions(pub(crate) Offers);

impl Sessions for TeamSessions {
    fn deliver<'a>(
        &'a self,
        card: &'a str,
        delivery: Delivery,
        text: String,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let no_session = || format!("No session {card} runs on this mesh");
            let (_, machine) = card.split_once('@').ok_or_else(no_session)?;
            let mesh = self.0.mesh.get().ok_or_else(no_session)?;
            let machine = find(&mesh, machine).ok_or_else(no_session)?;

            if machine.id == mesh.me() {
                return here(&self.0.agents, card, delivery, text).await;
            }

            let handed = Handed {
                card: card.to_owned(),
                delivery,
                text,
            };
            let request = Request {
                session: SessionId::default(),
                tool: Tool::TeamDeliver,
                args: serde_json::to_string(&handed).map_err(|e| e.to_string())?,
                approved: Approved::No,
            };

            match timeout(LIMIT, over_the_mesh(&mesh, machine.id, &request)).await {
                Ok(Ok(Reply::Done { failed: false, .. })) => Ok(()),
                Ok(Ok(Reply::Done { text, .. })) => Err(text),
                Ok(other) => {
                    log::info!("an ask did not reach {card}: {other:?}");
                    Err(format!("The machine that runs {card} did not take it"))
                }
                Err(_) => Err(format!(
                    "The machine that runs {card} did not answer in time"
                )),
            }
        })
    }
}

/// Puts `text` into the session on the card `card`, which runs here.
async fn here(agents: &Agents, card: &str, delivery: Delivery, text: String) -> Result<(), String> {
    let session = card.split_once('@').map_or(card, |(session, _)| session);
    let session = Into::<SessionId>::into(session);

    agents.deliver_from_teammate(&session, card, delivery, text).await
}
