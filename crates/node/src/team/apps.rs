//! What this mesh's devices ask of the team link besides the agents' tools.
//! A node hands a desktop the rosters it changed and the contacts' invites it
//! took (`Tool::TeamRoster`, `Tool::TeamInvite`). The apps read a board's
//! cards, dismiss them, tell teammates of a removal or a leave, invite a
//! contact and read the invites kept (`Tool::TeamApp`). What only a team's
//! gateway knows, or may send, goes on to that gateway once.

use super::{
    cards::Teammates,
    link::{TeamError, TeamLink},
};
use crate::{agents::mcp::peer_tools::Caller, peer_stream::over_the_mesh};
use beep_mesh::{Mesh, TeamWrites, take_roster};
use beep_proto::{
    SessionId,
    node::tools::{
        Approved, Reply, Request, TeamAppArgs, TeamAppCall, TeamCards, TeamInviteArgs,
        TeamRosterArgs, Tool,
    },
    team::{Card, Lane, TeamMsg, TeamRepo},
};
use iroh::{EndpointAddr, EndpointId};
use serde::{Serialize, de::DeserializeOwned};
use std::time::{Duration, SystemTime};
use tokio::time::timeout;

/// How long an invite may take to reach a contact, over all their devices.
const INVITE_LIMIT: Duration = Duration::from_secs(60);
/// How long a call sent on to the gateway may take, besides an invite's own time.
const HOP: Duration = Duration::from_secs(30);
/// What a call sent on to a machine that no longer acts for the team gets.
const MOVED: &str = "The team's gateway moved; try again";

/// A result for the caller, or why the call did not work.
type Outcome = Result<String, String>;

/// Answers `Tool::TeamRoster`, `Tool::TeamInvite` and `Tool::TeamApp`.
pub(crate) async fn answer(caller: &Caller, request: &Request) -> Reply {
    let Some(link) = &caller.team else {
        return reply(Err("This machine does not act for a team.".to_owned()));
    };
    let mesh = caller.mesh.as_ref();

    match request.tool {
        Tool::TeamRoster | Tool::TeamInvite if !caller.desktop_here => {
            reply(Err("Only a desktop records teams.".to_owned()))
        }
        Tool::TeamRoster => reply(roster(mesh, request).await),
        Tool::TeamInvite => reply(keep_invite(link, request)),
        _ => app(link, mesh, request).await,
    }
}

/// Writes a roster a node changed.
async fn roster(mesh: Option<&Mesh>, request: &Request) -> Outcome {
    let mesh = mesh.ok_or("This machine is in no mesh.")?;
    let TeamRosterArgs { record } = parse(&request.args)?;
    let repo = record.repo.clone();
    let held = TeamWrites::lock().await;
    take_roster(mesh, record, &held)
        .await
        .map_err(|e| e.to_string())?;

    Ok(format!("Recorded the team of {repo}."))
}

/// Keeps a contact's invite a node took, for this desktop to show.
fn keep_invite(link: &TeamLink, request: &Request) -> Outcome {
    let invite: TeamInviteArgs = parse(&request.args)?;
    link.keep_invite(invite)?;

    Ok("Kept the invite.".to_owned())
}

/// Answers a call about a team on its gateway; any other machine sends it
/// on to the gateway, once. A call sent on that reaches a machine which is
/// not the gateway in its own view is refused: the gateway moved.
async fn app(link: &TeamLink, mesh: Option<&Mesh>, request: &Request) -> Reply {
    let TeamAppCall { args, forwarded } = match parse(&request.args) {
        Ok(call) => call,
        Err(e) => return reply(Err(e)),
    };
    let repo = match &args {
        TeamAppArgs::Cards { repo }
        | TeamAppArgs::Dismiss { repo, .. }
        | TeamAppArgs::Remove { repo, .. }
        | TeamAppArgs::Leave { repo }
        | TeamAppArgs::Invite { via: repo, .. } => repo,
        TeamAppArgs::Invites | TeamAppArgs::DropInvite { .. } | TeamAppArgs::Queued { .. } => {
            return reply(here(link, args).await);
        }
    };

    let gateway = link.served(|served| served.gateways.get(repo).copied());

    match gateway {
        Some(gateway) if gateway == link.me() => reply(here(link, args).await),
        _ if forwarded => reply(Err(MOVED.to_owned())),
        Some(gateway) => match mesh {
            Some(mesh) => forward(mesh, gateway, args).await,
            None => reply(Err("This machine is in no mesh.".to_owned())),
        },
        None => reply(Err(
            "No machine of this mesh acts for the team yet; try again in a minute.".to_owned(),
        )),
    }
}

/// Sends `args` on to `gateway`, marked as sent on, within `HOP` (an
/// invite takes `INVITE_LIMIT` there besides).
async fn forward(mesh: &Mesh, gateway: EndpointId, args: TeamAppArgs) -> Reply {
    let limit = match args {
        TeamAppArgs::Invite { .. } => HOP + INVITE_LIMIT,
        _ => HOP,
    };
    let call = TeamAppCall {
        args,
        forwarded: true,
    };
    let request = match serde_json::to_string(&call) {
        Ok(args) => Request {
            session: SessionId::default(),
            tool: Tool::TeamApp,
            args,
            approved: Approved::No,
        },
        Err(e) => return reply(Err(e.to_string())),
    };

    match timeout(limit, over_the_mesh(mesh, gateway, &request)).await {
        Ok(Ok(answer)) => answer,
        Ok(Err(e)) => reply(Err(format!(
            "The machine that acts for the team did not answer: {e:#}"
        ))),
        Err(_) => reply(Err(
            "The machine that acts for the team did not answer in time".to_owned(),
        )),
    }
}

/// The answer of this device to an app's call.
async fn here(link: &TeamLink, args: TeamAppArgs) -> Outcome {
    match args {
        TeamAppArgs::Cards { repo } => json(&cards_here(link, &repo)),
        TeamAppArgs::Dismiss { repo, card } => {
            link.dismiss_card(&repo, &card);
            Ok("Dismissed.".to_owned())
        }
        TeamAppArgs::Remove { repo, login } => {
            let remove = TeamMsg::Remove {
                repo: repo.clone(),
                login,
            };
            tell(link, &repo, remove)
        }
        TeamAppArgs::Leave { repo } => {
            let own = link.home(|home| home.mesh);
            let login = link
                .with(|teams| {
                    let mut members = teams.rosters.get(&repo)?.members.iter();
                    members
                        .find(|member| Some(member.mesh) == own)
                        .map(|member| member.login.clone())
                })
                .ok_or_else(|| not_on(&repo))?;
            let leave = TeamMsg::Leave {
                repo: repo.clone(),
                login,
            };
            tell(link, &repo, leave)
        }
        TeamAppArgs::Invite {
            repo,
            ticket,
            devices,
            ..
        } => invite(link, &devices, TeamMsg::Invite { repo, ticket }).await,
        TeamAppArgs::Invites => json(&link.invites_waiting()),
        TeamAppArgs::DropInvite { invite } => {
            link.drop_invite(&invite);
            Ok("Dropped the invite.".to_owned())
        }
        TeamAppArgs::Queued { repo, issues } => {
            link.hand_queue(repo, issues);
            Ok("Kept the queue.".to_owned())
        }
    }
}

/// `repo`'s cards as this device, its gateway, holds them.
fn cards_here(link: &TeamLink, repo: &TeamRepo) -> TeamCards {
    let (followed, teammates, own_queued) = link.served(|served| {
        let teammates = served.teammates.get(repo);
        let followed: Vec<EndpointId> = teammates.into_iter().flat_map(Teammates::meshes).collect();
        let cards = served.boards.cards(repo).unwrap_or_default();
        let queued = |card: &Card| matches!(card.lane, Lane::Queued {});

        (
            followed,
            teammates.map(Teammates::cards).unwrap_or_default(),
            cards.into_iter().filter(queued).collect(),
        )
    });
    let (online, needs_you) = link.both(|teams, asks| {
        let members = teams.rosters.get(repo).map(|record| record.members.iter());
        let online = members
            .into_iter()
            .flatten()
            .filter(|member| followed.contains(&member.mesh))
            .map(|member| member.login.clone())
            .collect();

        (online, asks.cards(repo, SystemTime::now()))
    });

    TeamCards {
        needs_you,
        teammates,
        own_queued,
        online,
    }
}

/// The cards of the project named `project`, for the `team-cards` topic:
/// those of `Tool::TeamApp`'s `Cards`, from the machine that acts for its
/// team. `None` on any other machine, and for a project with no team here.
pub(crate) fn cards_of(link: &TeamLink, project: &str) -> Option<TeamCards> {
    let repo = link.home(|home| {
        home.projects
            .iter()
            .find(|(_, held)| held.name == project)
            .map(|(repo, _)| repo.clone())
    })?;
    let acts = link.served(|served| served.gateways.get(&repo) == Some(&link.me()));
    let team = link.with(|teams| teams.rosters.get(&repo).is_some());

    (acts && team).then(|| cards_here(link, &repo))
}

/// Sends `msg` to every member of `repo`'s team from another mesh, in the
/// background.
fn tell(link: &TeamLink, repo: &TeamRepo, msg: TeamMsg) -> Outcome {
    let own = link.home(|home| home.mesh);
    let others = link
        .with(|teams| {
            let members = teams.rosters.get(repo)?.members.iter();
            let others = members.filter(|member| Some(member.mesh) != own);
            Some(others.cloned().collect())
        })
        .ok_or_else(|| not_on(repo))?;
    link.tell(others, msg);

    Ok("Telling the team.".to_owned())
}

/// Sends a contact's first device that answers an invite, within
/// `INVITE_LIMIT`.
async fn invite(link: &TeamLink, devices: &[EndpointAddr], msg: TeamMsg) -> Outcome {
    let tries = async {
        for device in devices {
            match link.offer(std::slice::from_ref(device), &msg).await {
                Ok(None) => return Ok("Sent the invite.".to_owned()),
                Ok(Some(reason)) => return Err(reason),
                Err(e) => log::debug!("an invite did not reach {}: {e}", device.id.fmt_short()),
            }
        }

        Err(TeamError::Unreachable.to_string())
    };

    timeout(INVITE_LIMIT, tries)
        .await
        .unwrap_or_else(|_| Err("The contact did not answer in time".to_owned()))
}

fn not_on(repo: &TeamRepo) -> String {
    format!("This mesh is not on the team of {repo}")
}

fn parse<T: DeserializeOwned>(args: &str) -> Result<T, String> {
    serde_json::from_str(args).map_err(|e| format!("The arguments do not fit: {e}"))
}

fn json(value: &impl Serialize) -> Outcome {
    serde_json::to_string(value).map_err(|e| e.to_string())
}

fn reply(outcome: Outcome) -> Reply {
    match outcome {
        Ok(text) => Reply::Done {
            text,
            failed: false,
        },
        Err(text) => Reply::Done { text, failed: true },
    }
}

#[cfg(test)]
#[path = "../tests/team_apps.rs"]
mod tests;
