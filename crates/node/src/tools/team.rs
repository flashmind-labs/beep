//! The team tools, for sessions in a team project: `team` lists the team,
//! `team_ask` asks a teammate's agent or a teammate, or answers an ask. The
//! session's node makes the call and sends it to the machine of its mesh
//! that acts for the team (`team::gateway`), which answers it; that machine
//! hands asks and answers to the sessions they are for (`Tool::TeamDeliver`),
//! and has a machine with the project start a session on an issue newly
//! assigned to the user (`Tool::TeamPickup`).

use super::{Node, desktop, server::ASK_LIMIT};
use crate::agents::child_view::full_id;
use crate::agents::machine_list::{find, machine_name};
use crate::agents::reply::{fail, json_of, ok, parse};
use crate::agents::sessions;
use crate::peer_stream::over_the_mesh;
use crate::{
    agents::{Agents, Answered, Asking},
    team::{
        asks::{Delivery, MAX_TEXT, Outgoing},
        cards::Teammates,
        link::TeamLink,
        pickup::{PickupStart, start_issue},
    },
};
use beep_mesh::{Mesh, TeamTicket};
use beep_proto::{
    SessionId,
    node::{
        agent::{AnswerOption, OptionKind},
        tools::{Approved, Reply, Request, Tool},
    },
    team::{AgentRef, Card, Target, TeamRepo},
};
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
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

/// `team_join` as an agent calls it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JoinToolArgs {
    ticket: String,
    machine: Option<String>,
    folder: Option<String>,
}

const JOIN_CONFIRM: &str = "join";
const JOIN_PRIVACY: &str = "Teammates see your cards on this project's board and can message your \
    agents. They never see your sessions, files, terminals or other projects.";

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
pub(super) fn team_of(node: &Node, session: &SessionId) -> Option<(TeamLink, TeamRepo)> {
    let link = node.team.clone();
    let folder = node.agents.sessions.folder_of(session)?;
    let repo = link.home(|home| home.team_of(&folder))?;
    link.with(|teams| teams.rosters.get(&repo).is_some())
        .then_some(())?;

    Some((link, repo))
}

/// `team_join` from any session: validates the code, asks the user to confirm,
/// and forwards the join (with automatic project import or clone) to a desktop.
pub(super) async fn join_call(node: &Node, request: &Request) -> Reply {
    let args: JoinToolArgs = match parse(&request.args) {
        Ok(args) => args,
        Err(e) => return fail(e),
    };
    let ticket_text = args.ticket.trim().to_owned();
    let ticket = match TeamTicket::parse(&ticket_text) {
        Ok(ticket) => ticket,
        Err(e) => return fail(e.to_string()),
    };
    let machine = args
        .machine
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let folder = args
        .folder
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let mut detail = format!(
        "Adds {} automatically if it is not imported yet, proves push access, and joins its team.",
        ticket.repo
    );
    if let Some(machine) = machine {
        detail.push_str(&format!("\nMachine: {machine}"));
    }
    if let Some(folder) = folder {
        detail.push_str(&format!("\nFolder: {folder}"));
    }
    detail.push_str(&format!("\n\n{JOIN_PRIVACY}"));

    let question = Asking {
        text: format!("Join the team for {}?", ticket.repo),
        detail: Some(detail),
        options: vec![
            AnswerOption {
                id: JOIN_CONFIRM.to_owned(),
                name: "Join".to_owned(),
                kind: OptionKind::AllowOnce,
            },
            AnswerOption {
                id: "cancel".to_owned(),
                name: "Cancel".to_owned(),
                kind: OptionKind::RejectOnce,
            },
        ],
    };

    match timeout(
        ASK_LIMIT,
        node.agents.sessions.ask(&request.session, question),
    )
    .await
    {
        Ok(Ok(Answered::Picked(option))) if option.id == JOIN_CONFIRM => {}
        Ok(Ok(_)) => {
            return fail("The user did not join the team. Do not try again unless they ask.");
        }
        Ok(Err(e)) => return fail(format!("Could not ask the user: {e:#}")),
        Err(_) => return fail("Not joined: the user did not answer within an hour."),
    }

    let mut payload = json!({
        "action": "join_team",
        "ticket": ticket_text,
    });
    if let Some(machine) = machine {
        payload["machine"] = json!(machine);
    }
    if let Some(folder) = folder {
        payload["folder"] = json!(folder);
    }

    let forward = Request {
        session: request.session.clone(),
        tool: Tool::Projects,
        args: payload.to_string(),
        approved: Approved::No,
    };

    desktop::call(node, &forward).await
}

/// `team` or `team_ask` from a session on this node, answered by the
/// machine of the mesh that acts for the team.
pub(super) async fn call(node: &Node, request: &Request) -> Reply {
    let (Some(mesh), Some((link, repo))) = (node.mesh.get(), team_of(node, &request.session))
    else {
        return fail("This session's project has no team.");
    };
    let (args, asking) = match request.tool {
        Tool::TeamAsk => match outgoing(&node.agents, &mesh, &link, request, repo.clone()) {
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
        true => link.asks(|asks| asks.limit.take(&request.session, Instant::now())),
        false => Ok(()),
    };
    if let Err(e) = counted {
        return fail(e);
    }

    let gateway = link.served(|served| served.gateways.get(&repo).copied());
    let reply = match gateway {
        Some(gateway) if gateway == mesh.me() => answer(&link, &node.agents, None, &request).await,
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
        link.asks(|asks| asks.limit.give_back(&request.session));
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
        .sessions
        .get(&request.session)
        .ok_or("This session no longer runs.")?;
    let card = full_id(
        sessions::name_of(&session).as_ref(),
        &machine_name(Some(mesh)),
    );
    let own = link.home(|home| home.mesh);
    let (login, to) = link.with(|teams| {
        let record = teams
            .rosters
            .get(&repo)
            .ok_or("This project has no team.")?;
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
/// `team_ask`, of a session's machine to `TeamDeliver`, and of a machine
/// with the project to `TeamPickup`. `from` is the node of the mesh that
/// called, or `None` for this machine and the mesh's desktops.
pub(crate) async fn answer(
    link: &TeamLink,
    agents: &Agents,
    from: Option<EndpointId>,
    request: &Request,
) -> Reply {
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
        Tool::TeamPickup => {
            let order = match parse::<PickupStart>(&request.args) {
                Ok(order) => order,
                Err(e) => return fail(e),
            };
            if let Err(e) = acts_for(link, &order.repo, from) {
                return fail(e);
            }
            let Some(mesh) = agents.links.mesh() else {
                return fail("This machine is in no mesh.");
            };

            match start_issue(agents, &mesh, &order).await {
                Ok(auto) => json_of(&auto),
                Err(e) => fail(e),
            }
        }
        Tool::TeamDeliver => match parse::<Handed>(&request.args) {
            Ok(handed) => match handed_here(link, agents, from, handed).await {
                Ok(()) => ok("Delivered."),
                Err(e) => fail(e),
            },
            Err(e) => fail(e),
        },
        other => fail(format!("{other} is not a team tool.")),
    }
}

/// Whether `from` may act for `repo`'s team here: the machine this machine
/// elects as the team's gateway, or this machine or a desktop (`None`).
fn acts_for(link: &TeamLink, repo: &TeamRepo, from: Option<EndpointId>) -> Result<(), String> {
    let Some(from) = from else {
        return Ok(());
    };

    let gateway = link.served(|served| served.gateways.get(repo).copied());

    match gateway {
        Some(gateway) if gateway == from => Ok(()),
        _ => Err(format!(
            "{} does not act for the team of {repo} here.",
            from.fmt_short()
        )),
    }
}

/// The team of `repo`: its members, this mesh's and the teammates' cards,
/// and its open issues with their assignees.
async fn team(link: &TeamLink, repo: &TeamRepo) -> Reply {
    let view = link
        .served(|served| {
            let ours = served.boards.cards(repo)?;
            let theirs = served.teammates.get(repo).map(Teammates::cards);

            Some(
                ours.into_iter()
                    .chain(theirs.unwrap_or_default())
                    .collect::<Vec<Card>>(),
            )
        })
        .and_then(|cards| {
            let members = link.with(|teams| {
                let record = teams.rosters.get(repo)?;
                Some(
                    record
                        .members
                        .iter()
                        .map(|member| member.login.to_string())
                        .collect::<Vec<String>>(),
                )
            })?;

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

/// Hands `text` to the session on `card`, here or on the machine of this
/// mesh that runs it.
pub(super) async fn deliver(
    node: &Node,
    card: &str,
    delivery: Delivery,
    text: String,
) -> Result<(), String> {
    let no_session = || format!("No session {card} runs on this mesh");
    let (_, machine) = card.split_once('@').ok_or_else(no_session)?;
    let mesh = node.mesh.get().ok_or_else(no_session)?;
    let machine = find(&mesh, machine).ok_or_else(no_session)?;

    if machine.id == mesh.me() {
        return here(&node.agents, card, delivery, text).await;
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
}

/// Puts what `from` handed into its session here, when that session works
/// in a team project and `from` acts for that team; an ask also needs the
/// team to take messages from teammates' agents. Whether an answer is for
/// an ask the session sent only the gateway knows.
async fn handed_here(
    link: &TeamLink,
    agents: &Agents,
    from: Option<EndpointId>,
    handed: Handed,
) -> Result<(), String> {
    let Handed {
        card,
        delivery,
        text,
    } = handed;
    let session = agents
        .teammates_session(&session_of(&card))
        .ok_or_else(|| format!("{card} no longer runs"))?;
    let folder = PathBuf::from(session.info().cwd);
    let repo = link.home(|home| home.team_of(&folder));
    let team = repo.and_then(|repo| {
        let asks = link.with(|teams| teams.rosters.get(&repo).map(|record| record.asks))?;
        Some((repo, asks))
    });
    let (repo, asks) = team.ok_or_else(|| format!("{card} works on no team's project"))?;

    acts_for(link, &repo, from)?;
    if delivery == Delivery::Ask && !asks {
        return Err(format!("{card} takes no messages from teammates' agents"));
    }

    here(agents, &card, delivery, text).await
}

/// Puts `text` into the session on the card `card`, which runs here.
async fn here(agents: &Agents, card: &str, delivery: Delivery, text: String) -> Result<(), String> {
    agents
        .deliver_from_teammate(&session_of(card), card, delivery, text)
        .await
}

/// The session of the card `card` (`<session>@<machine>`).
fn session_of(card: &str) -> SessionId {
    let session = card.split_once('@').map_or(card, |(session, _)| session);

    Into::<SessionId>::into(session)
}
